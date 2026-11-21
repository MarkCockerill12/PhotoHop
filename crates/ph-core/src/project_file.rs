//! `.phop` project files: pretty JSON with media paths stored relative to the project folder,
//! plus an `<name>.assets/` folder next to it for paint-layer rasters and baked AI results.

use crate::doc::{FORMAT_VERSION, LayerId, Project};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "phop";

/// Folder holding sidecar files (paint rasters, baked masks) for a project file.
pub fn assets_dir(project_path: &Path) -> PathBuf {
    let stem = project_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    project_path.with_file_name(format!("{stem}.assets"))
}

pub fn paint_layer_path(project_path: &Path, id: LayerId) -> PathBuf {
    assets_dir(project_path).join(format!("paint-{}.png", id.0))
}

pub fn normals_path(project_path: &Path, id: LayerId) -> PathBuf {
    assets_dir(project_path).join(format!("normals-{}.png", id.0))
}

pub fn depth_path(project_path: &Path, id: LayerId) -> PathBuf {
    assets_dir(project_path).join(format!("depth-{}.png", id.0))
}

pub fn mask_path(project_path: &Path, id: LayerId) -> PathBuf {
    assets_dir(project_path).join(format!("mask-{}.png", id.0))
}

fn relativize(path: &Path, base: &Path) -> PathBuf {
    path.strip_prefix(base).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf())
}

pub fn save(doc: &Project, path: &Path) -> Result<()> {
    let base = path.parent().unwrap_or(Path::new("."));
    let mut out = doc.clone();
    for m in &mut out.media {
        m.path = relativize(&m.path, base);
    }
    for c in [&mut out.camera_solve, &mut out.body_motion].into_iter().flatten() {
        *c = relativize(c, base);
    }
    let json = serde_json::to_string_pretty(&out)?;
    let tmp = path.with_extension("phop.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Project> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: Project = serde_json::from_str(&text).context("parsing project file")?;
    if doc.format_version > FORMAT_VERSION {
        bail!("project was saved by a newer PhotoHop (format {})", doc.format_version);
    }
    let base = path.parent().unwrap_or(Path::new("."));
    for m in &mut doc.media {
        if m.path.is_relative() {
            m.path = base.join(&m.path);
        }
    }
    for c in [&mut doc.camera_solve, &mut doc.body_motion].into_iter().flatten() {
        if c.is_relative() {
            *c = base.join(&*c);
        }
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{MediaId, MediaKind, MediaRef};

    #[test]
    fn roundtrip_relative_paths() {
        let dir = std::env::temp_dir().join(format!("phop-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Project::new("t", 64, 64, 24.0, 1.0);
        doc.media.push(MediaRef {
            id: MediaId::new(),
            path: dir.join("a.png"),
            kind: MediaKind::Image,
            width: 4,
            height: 4,
        });
        let file = dir.join("p.phop");
        save(&doc, &file).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("\"a.png\""));
        let back = load(&file).unwrap();
        assert_eq!(back, doc);
        std::fs::remove_dir_all(&dir).ok();
    }
}
