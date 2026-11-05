use crate::{
    format::format_size,
    layout::{Node, Tile, fit_label, layout_tree, text_color, wrap_label},
};
use gpui::{prelude::*, *};
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Default)]
pub struct MapCache {
    pub bounds: Bounds<Pixels>,
    pub tiles: Vec<Tile>,
    key: Option<(u64, f32, f32)>,
    first_paint: bool,
    startup_reported: bool,
}

pub fn color(hex: &str) -> Hsla {
    rgb(u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x94a3b5)).into()
}

pub fn category_color(category: Option<&str>) -> String {
    static CATEGORIES: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let categories = CATEGORIES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/categories.json")).expect("category data")
    });
    categories
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_str() == category)
        .and_then(|c| c["color"].as_str())
        .unwrap_or("#94a3b5")
        .into()
}

fn line(text: &str, font_size: f32, ink: Hsla, bold: bool, window: &Window) -> ShapedLine {
    let mut font = font("Segoe UI");
    font.weight = if bold {
        FontWeight::SEMIBOLD
    } else {
        FontWeight::MEDIUM
    };
    window.text_system().shape_line(
        text.to_owned().into(),
        px(font_size),
        &[TextRun {
            len: text.len(),
            font,
            color: ink,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub fn map(
    tree: Rc<Node>,
    version: u64,
    cache: Rc<RefCell<MapCache>>,
    hover: Option<usize>,
    trace: bool,
    startup: Instant,
) -> impl IntoElement {
    let pre = cache.clone();
    canvas(
        move |bounds, _, _| {
            let mut cache = pre.borrow_mut();
            let key = (
                version,
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            cache.bounds = bounds;
            if cache.key != Some(key) {
                let start = Instant::now();
                cache.tiles = layout_tree(&tree, key.1 as f64, key.2 as f64, true);
                cache.key = Some(key);
                cache.first_paint = true;
                if trace {
                    eprintln!(
                        "layout_ms={:.3} tile_count={}",
                        start.elapsed().as_secs_f64() * 1000.,
                        cache.tiles.len()
                    );
                }
            }
        },
        move |bounds, (), window, cx| {
            let start = Instant::now();
            let mut cache = cache.borrow_mut();
            for tile in &cache.tiles {
                let rect = Bounds::new(
                    bounds.origin + point(px(tile.x as f32), px(tile.y as f32)),
                    size(px(tile.width as f32), px(tile.height as f32)),
                );
                let fill_color = category_color(tile.node.category.as_deref());
                window.paint_quad(fill(
                    rect,
                    if tile.frame {
                        color("#354557")
                    } else {
                        color(&fill_color)
                    },
                ));
                let frame = tile.frame;
                if if frame {
                    tile.width > 40. && tile.height > 19.
                } else {
                    tile.width >= 72. && tile.height >= 44.
                } {
                    let prefix = if tile.node.kind == sprawl_core::scanner::Kind::Dir {
                        "▱ "
                    } else {
                        ""
                    };
                    let ink = if frame {
                        color("#eaf0f6")
                    } else {
                        color(text_color(&fill_color))
                    };
                    let measure =
                        |text: &str| f32::from(line(text, 12., ink, frame, window).width) as f64;
                    let width = tile.width - 16. - measure(prefix);
                    let labels = if frame {
                        fit_label(&tile.node.name, width, measure)
                            .into_iter()
                            .collect()
                    } else {
                        wrap_label(
                            &tile.node.name,
                            width,
                            if tile.height >= 62. { 2 } else { 1 },
                            measure,
                        )
                    };
                    let size_label = format_size(tile.node.size as f64);
                    if labels.is_empty()
                        || (!frame
                            && f32::from(line(&size_label, 11., ink, false, window).width) as f64
                                > tile.width - 14.)
                    {
                        continue;
                    }
                    window.with_content_mask(Some(ContentMask { bounds: rect }), |window| {
                        for (i, label) in labels.iter().enumerate() {
                            let label = format!("{}{label}", if i == 0 { prefix } else { "" });
                            let _ = line(&label, 12., ink, frame, window).paint(
                                rect.origin + point(px(7.), px(2. + i as f32 * 15.)),
                                px(15.),
                                window,
                                cx,
                            );
                        }
                        if !frame {
                            let _ = line(&size_label, 11., ink, false, window).paint(
                                rect.origin + point(px(7.), px(3. + labels.len() as f32 * 15.)),
                                px(15.),
                                window,
                                cx,
                            );
                        }
                    });
                }
            }
            if let Some(index) = hover
                && let Some(hovered) = cache.tiles.get(index)
                && let Some(id) = hovered.node.id
            {
                for tile in &cache.tiles {
                    if tile.node.id == Some(id) || tile.branch == Some(id) {
                        let rect = Bounds::new(
                            bounds.origin + point(px(tile.x as f32 + 1.), px(tile.y as f32 + 1.)),
                            size(px((tile.width - 2.) as f32), px((tile.height - 2.) as f32)),
                        );
                        window.paint_quad(quad(
                            rect,
                            px(0.),
                            transparent_black(),
                            px(if tile.node.id == Some(id) { 2. } else { 0.7 }),
                            color("#eaf0f6"),
                            BorderStyle::default(),
                        ));
                    }
                }
            }
            if trace && cache.first_paint {
                eprintln!(
                    "treemap_paint_ms={:.3} tile_count={}",
                    start.elapsed().as_secs_f64() * 1000.,
                    cache.tiles.len()
                );
                if !cache.startup_reported {
                    eprintln!(
                        "startup_to_treemap_ms={:.3}",
                        startup.elapsed().as_secs_f64() * 1000.
                    );
                    cache.startup_reported = true;
                }
            }
            cache.first_paint = false;
        },
    )
    .size_full()
}
