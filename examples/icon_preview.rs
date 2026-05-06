//! Rasterize `assets/icon.svg` to PNGs at icon-relevant sizes.
//!
//! Usage: `cargo run --example icon_preview`
//! Output: `target/icon-preview/icon-<N>.png` for N in {16,32,48,64,128,256,512}.
//! Tip: open the directory in Finder/your file manager and use the gallery view
//! to see every size at once. The 16/32px renders are the ones that decide
//! whether the icon survives in the dock and taskbar.

use std::fs;
use std::path::PathBuf;

use resvg::render;
use tiny_skia::{Pixmap, Transform};
use usvg::{Options, Tree};

const SIZES: &[u32] = &[16, 32, 48, 64, 128, 256, 512];

fn main() -> anyhow::Result<()> {
    let svg_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icon.svg");
    let svg_data = fs::read(&svg_path)?;

    let tree = Tree::from_data(&svg_data, &Options::default())?;
    let svg_size = tree.size();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/icon-preview");
    fs::create_dir_all(&out_dir)?;

    for &size in SIZES {
        let scale = size as f32 / svg_size.width().max(svg_size.height());
        let mut pixmap = Pixmap::new(size, size)
            .ok_or_else(|| anyhow::anyhow!("failed to allocate {size}x{size} pixmap"))?;

        render(
            &tree,
            Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );

        let out = out_dir.join(format!("icon-{size}.png"));
        pixmap.save_png(&out)?;
        println!("wrote {}", out.display());
    }

    println!("\nopen {} to review", out_dir.display());
    Ok(())
}
