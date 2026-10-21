//! Color links between pieces.
//!
//! A piece whose color type is `Link` follows another piece's color: the
//! target can be in the same crosshair or in any other saved crosshair.
//! Everything downstream (rendering, mask/cfg export) works on *resolved*
//! pieces, in which every link has been replaced by the target's color, and
//! every Dynamic piece carries a group number:
//!
//! - each unlinked Dynamic piece is its own effect group, implicitly;
//! - pieces linked to the same Dynamic piece share its group.

use crate::types::{ColorType, DynamicEffects, LinkTarget, Piece};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// How deep a chain of links may go before it counts as a cycle.
const MAX_LINK_DEPTH: usize = 16;

/// Saved crosshairs by file stem, for links that point outside the current
/// crosshair.  Re-scanned now and then so edits elsewhere show up.
#[derive(Default)]
pub struct LinkLibrary {
    crosshairs: Vec<(String, Vec<Piece>)>,
    last_scan: Option<Instant>,
}

impl LinkLibrary {
    /// Re-read the project directory if it has not been read recently.
    pub fn refresh_if_stale(&mut self) {
        let stale = self
            .last_scan
            .map_or(true, |t| t.elapsed() > Duration::from_secs(3));
        if stale {
            self.refresh();
        }
    }

    /// Re-read every saved crosshair now (call before exporting).
    pub fn refresh(&mut self) {
        self.last_scan = Some(Instant::now());
        self.crosshairs.clear();
        let dir = crate::project_io::project_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            if let Ok(project) = crate::project_io::load_project(&path) {
                self.crosshairs.push((stem, project.pieces));
            }
        }
    }

    pub fn crosshairs(&self) -> &[(String, Vec<Piece>)] {
        &self.crosshairs
    }

    fn get(&self, stem: &str) -> Option<&[Piece]> {
        self.crosshairs
            .iter()
            .find(|(s, _)| s == stem)
            .map(|(_, p)| p.as_slice())
    }
}

/// Pieces with every link resolved, plus the effects of each dynamic group.
pub struct Resolved {
    pub pieces: Vec<Piece>,
    /// Indexed by the `group` of `ColorType::Dynamic` in `pieces`.
    pub groups: Vec<DynamicEffects>,
}

/// A fresh id: unique within the process and effectively unique across files.
fn fresh_uid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    // splitmix64 finalizer
    let mut z = t ^ n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    // keep clear of the small deterministic ids and of 0
    z | (1 << 40)
}

/// Give every piece a unique non-zero id.  Pieces without one get a stable
/// position-based id when free (so files that were never saved with ids keep
/// the same ids on every load), otherwise a fresh one; duplicated ids (a
/// copied piece) are replaced on the later copy.  Returns true if anything
/// changed.
pub fn assign_uids(pieces: &mut [Piece]) -> bool {
    let mut used: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut changed = false;
    // keep existing unique ids first so they never move
    let mut needs: Vec<usize> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let id = p.uid();
        if id == 0 || !used.insert(id) {
            needs.push(i);
        }
    }
    for i in needs {
        let positional = i as u64 + 1;
        let id = if used.insert(positional) { positional } else {
            let mut id = fresh_uid();
            while !used.insert(id) {
                id = fresh_uid();
            }
            id
        };
        pieces[i].set_uid(id);
        changed = true;
    }
    changed
}

/// Convert files from before per-piece effects: the project-level effect
/// chains by group number become each Dynamic piece's own `effects`, and
/// pieces that shared a group are linked to the first one of it.
pub fn migrate_legacy_groups(pieces: &mut [Piece], groups: &[DynamicEffects]) {
    let mut first_of_group: HashMap<u32, u64> = HashMap::new();
    for p in pieces.iter_mut() {
        let uid = p.uid();
        let ct = p.color_type_mut();
        if let ColorType::Dynamic { effects, group, .. } = ct {
            let g = *group;
            let chain = groups.get(g as usize).or(groups.first()).cloned().unwrap_or_default();
            match first_of_group.get(&g) {
                None => {
                    first_of_group.insert(g, uid);
                    *effects = chain;
                    *group = 0;
                }
                Some(&first) => {
                    *ct = ColorType::Link {
                        target: LinkTarget { crosshair: None, piece: first },
                    };
                }
            }
        }
    }
}

/// Where a link chain ends: the piece whose color is actually used.
struct Root {
    /// Crosshair the root lives in (`None` = the current one) and its id.
    key: (Option<String>, u64),
    color: String,
    color_type: ColorType,
}

fn find<'a>(pieces: &'a [Piece], uid: u64) -> Option<&'a Piece> {
    if uid == 0 {
        return None;
    }
    pieces.iter().find(|p| p.uid() == uid)
}

fn follow(
    pieces: &[Piece],
    lib: &LinkLibrary,
    mut ctx: Option<String>,
    mut target: LinkTarget,
) -> Option<Root> {
    for _ in 0..MAX_LINK_DEPTH {
        if target.crosshair.is_some() {
            ctx = target.crosshair.clone();
        }
        let list: &[Piece] = match &ctx {
            None => pieces,
            Some(stem) => lib.get(stem)?,
        };
        let p = find(list, target.piece)?;
        match p.color_type() {
            ColorType::Link { target: next } => {
                // a link inside another crosshair is relative to that crosshair
                target = next;
            }
            ct => {
                return Some(Root {
                    key: (ctx, p.uid()),
                    color: p.base_color(),
                    color_type: ct,
                })
            }
        }
    }
    None // cycle
}

/// Replace every link with the color it follows and number the dynamic groups.
pub fn resolve(pieces: &[Piece], lib: &LinkLibrary) -> Resolved {
    let mut out: Vec<Piece> = pieces.to_vec();
    let mut groups: Vec<DynamicEffects> = Vec::new();
    let mut group_of: HashMap<(Option<String>, u64), u32> = HashMap::new();

    let mut group_for = |key: (Option<String>, u64), effects: &DynamicEffects| -> u32 {
        *group_of.entry(key).or_insert_with(|| {
            groups.push(effects.clone());
            groups.len() as u32 - 1
        })
    };

    for (i, piece) in out.iter_mut().enumerate() {
        match piece.color_type() {
            ColorType::Link { target } => {
                match follow(pieces, lib, None, target) {
                    Some(root) => {
                        piece.set_color_override(&root.color);
                        let mut ct = root.color_type;
                        if let ColorType::Dynamic { effects, group, .. } = &mut ct {
                            *group = group_for(root.key, effects);
                        }
                        *piece.color_type_mut() = ct;
                    }
                    None => {
                        // missing target or a cycle: plain white so it is noticed
                        piece.set_color_override("#ffffffff");
                        *piece.color_type_mut() = ColorType::Solid;
                    }
                }
            }
            ColorType::Dynamic { effects, .. } => {
                // pieces without an id yet are keyed by position
                let id = match piece.uid() {
                    0 => u64::MAX - i as u64,
                    id => id,
                };
                let g = group_for((None, id), &effects);
                if let ColorType::Dynamic { group, .. } = piece.color_type_mut() {
                    *group = g;
                }
            }
            _ => {}
        }
    }

    Resolved { pieces: out, groups }
}

/// Short human label for a piece in link pickers: "#2 Cross".
pub fn piece_label(index: usize, piece: &Piece) -> String {
    let kind = match piece.color_type() {
        ColorType::Solid => "",
        ColorType::Eraser => " · eraser",
        ColorType::Rainbow { .. } => " · rainbow",
        ColorType::GradientCycle { .. } => " · gradient",
        ColorType::Dynamic { .. } => " · dynamic",
        ColorType::Link { .. } => " · link",
    };
    format!("#{} {}{}", index + 1, piece.type_name(), kind)
}

/// Stem of a project file path, for telling "this crosshair" apart in the library.
pub fn stem_of(path: &Path) -> Option<String> {
    path.file_stem().map(|s| s.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::default_cross_piece;

    fn dynamic(uid: u64) -> Piece {
        let mut p = default_cross_piece();
        p.set_uid(uid);
        *p.color_type_mut() = ColorType::Dynamic {
            effects: DynamicEffects::default(),
            group: 0,
            _legacy_mode: None,
            _legacy_modes: None,
            _legacy_tint: None,
            _legacy_strength: None,
        };
        p
    }

    fn link_to(uid: u64, target: u64) -> Piece {
        let mut p = default_cross_piece();
        p.set_uid(uid);
        *p.color_type_mut() = ColorType::Link {
            target: LinkTarget { crosshair: None, piece: target },
        };
        p
    }

    fn group(p: &Piece) -> u32 {
        match p.color_type() {
            ColorType::Dynamic { group, .. } => group,
            other => panic!("not dynamic: {other:?}"),
        }
    }

    #[test]
    fn unlinked_dynamic_pieces_get_their_own_group() {
        let r = resolve(&[dynamic(1), dynamic(2), dynamic(3)], &LinkLibrary::default());
        assert_eq!(r.groups.len(), 3);
        assert_ne!(group(&r.pieces[0]), group(&r.pieces[1]));
    }

    #[test]
    fn linked_dynamic_pieces_share_a_group_and_follow_effects() {
        let mut a = dynamic(1);
        if let ColorType::Dynamic { effects, .. } = a.color_type_mut() {
            effects.invert.enabled = true;
        }
        let r = resolve(&[a, dynamic(2), link_to(3, 1)], &LinkLibrary::default());
        assert_eq!(r.groups.len(), 2);
        assert_eq!(group(&r.pieces[0]), group(&r.pieces[2]));
        assert!(r.groups[group(&r.pieces[2]) as usize].invert.enabled);
    }

    #[test]
    fn link_follows_a_solid_color_and_chains() {
        let mut a = default_cross_piece();
        a.set_uid(1);
        a.set_color_override("#123456ff");
        let r = resolve(&[a, link_to(2, 1), link_to(3, 2)], &LinkLibrary::default());
        assert_eq!(r.pieces[2].base_color(), "#123456ff");
        assert_eq!(r.pieces[2].color_type(), ColorType::Solid);
    }

    #[test]
    fn cycles_and_missing_targets_resolve_to_white() {
        let r = resolve(&[link_to(1, 2), link_to(2, 1), link_to(3, 99)], &LinkLibrary::default());
        for p in &r.pieces {
            assert_eq!(p.base_color(), "#ffffffff");
            assert_eq!(p.color_type(), ColorType::Solid);
        }
    }

    #[test]
    fn uids_are_unique_and_copies_get_new_ones() {
        let mut ps = vec![default_cross_piece(), default_cross_piece()];
        ps[0].set_uid(7);
        ps[1].set_uid(7); // a duplicate
        assert!(assign_uids(&mut ps));
        assert_eq!(ps[0].uid(), 7);
        assert_ne!(ps[1].uid(), 7);
        assert_ne!(ps[1].uid(), 0);
        assert!(!assign_uids(&mut ps));
    }

    #[test]
    fn legacy_groups_migrate_to_per_piece_effects_and_links() {
        let mut g0 = DynamicEffects::default();
        g0.invert.enabled = true;
        let mut g1 = DynamicEffects::default();
        g1.saturate.enabled = true;
        let mut ps = vec![dynamic(1), dynamic(2), dynamic(3)];
        for (p, g) in ps.iter_mut().zip([0u32, 1, 0]) {
            if let ColorType::Dynamic { group, .. } = p.color_type_mut() {
                *group = g;
            }
        }
        migrate_legacy_groups(&mut ps, &[g0, g1]);
        let r = resolve(&ps, &LinkLibrary::default());
        assert_eq!(r.groups.len(), 2);
        assert_eq!(group(&r.pieces[0]), group(&r.pieces[2]));
        assert!(r.groups[group(&r.pieces[0]) as usize].invert.enabled);
        assert!(r.groups[group(&r.pieces[1]) as usize].saturate.enabled);
    }
}
