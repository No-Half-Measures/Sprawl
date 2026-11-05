use crate::{format::*, layout::*};
use serde_json::Value;

#[test]
fn golden_layout() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/layout.json")).unwrap();
    for c in cases.as_array().unwrap() {
        let tree: Node = serde_json::from_value(c["tree"].clone()).unwrap();
        let tiles = layout_tree(
            &tree,
            c["width"].as_f64().unwrap(),
            c["height"].as_f64().unwrap(),
            c["frames"].as_bool().unwrap(),
        );
        let expected = c["tiles"].as_array().unwrap();
        assert_eq!(tiles.len(), expected.len(), "{}", c["name"]);
        for (tile, want) in tiles.iter().zip(expected) {
            let got = serde_json::to_value(tile).unwrap();
            for field in ["x", "y", "width", "height"] {
                assert!(
                    (got[field].as_f64().unwrap() - want[field].as_f64().unwrap()).abs() <= 1e-6,
                    "{} {field}: {} != {}",
                    c["name"],
                    got[field],
                    want[field]
                );
            }
            for field in ["depth", "ancestors", "branch", "frame"] {
                assert_eq!(got[field], want[field], "{} {field}", c["name"]);
            }
            assert_eq!(got["node"]["id"], want["node"]["id"]);
        }
    }
}

#[test]
fn typescript_labels_and_colors() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/format.json")).unwrap();
    for c in cases["labels"].as_array().unwrap() {
        let measure = |s: &str| s.chars().count() as f64 * 7.;
        let name = c["name"].as_str().unwrap();
        let width = c["width"].as_f64().unwrap();
        assert_eq!(
            serde_json::to_value(fit_label(name, width, measure)).unwrap(),
            c["fit"]
        );
        assert_eq!(
            serde_json::to_value(wrap_label(
                name,
                width,
                c["lines"].as_u64().unwrap() as usize,
                measure
            ))
            .unwrap(),
            c["wrap"]
        );
    }
    for c in cases["colors"].as_array().unwrap() {
        assert_eq!(text_color(c["hex"].as_str().unwrap()), c["result"]);
    }
}

#[test]
fn typescript_formatters() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/format.json")).unwrap();
    for c in cases["sizes"].as_array().unwrap() {
        assert_eq!(format_size(c["value"].as_f64().unwrap()), c["result"]);
    }
    for c in cases["shares"].as_array().unwrap() {
        assert_eq!(
            percent(c["value"].as_f64().unwrap(), c["total"].as_f64().unwrap()),
            c["result"]
        );
    }
    for c in cases["counts"].as_array().unwrap() {
        assert_eq!(count(c["value"].as_u64().unwrap(), "file"), c["result"]);
    }
    for c in cases["paths"].as_array().unwrap() {
        assert_eq!(
            middle_path(
                c["value"].as_str().unwrap(),
                c["limit"].as_u64().unwrap() as usize
            ),
            c["result"]
        );
    }
    assert_eq!(format_size(f64::NAN), "0 B");
    assert_eq!(percent(f64::INFINITY, 1.), "0%");
}

#[test]
fn hit_prefers_last_tile_and_includes_edges() {
    let tree: Node = serde_json::from_value(serde_json::json!({"id":0,"name":"root","kind":"dir","size":100,"children":[{"id":1,"name":"folder","kind":"dir","size":100,"children":[{"id":2,"name":"file","kind":"file","size":100}]}]})).unwrap();
    let tiles = layout_tree(&tree, 400., 300., true);
    let last = tiles.last().unwrap();
    assert_eq!(hit(&tiles, last.x, last.y), Some(tiles.len() - 1));
    assert_eq!(hit(&tiles, -1., 0.), None);
}

#[test]
fn typescript_results_helpers() {
    let cases: Value =
        serde_json::from_str(include_str!("../tests/fixtures/results.json")).unwrap();
    let now = cases["now"].as_u64().unwrap();
    for c in cases["deltas"].as_array().unwrap() {
        assert_eq!(format_delta(c["value"].as_f64().unwrap()), c["result"]);
    }
    for c in cases["dates"].as_array().unwrap() {
        assert_eq!(format_date(c["value"].as_u64().unwrap()), c["result"]);
    }
    for c in cases["scanDates"].as_array().unwrap() {
        assert_eq!(
            format_scan_date_at(
                c["value"].as_u64().unwrap(),
                now,
                chrono::FixedOffset::east_opt(cases["offset"].as_i64().unwrap() as i32).unwrap()
            ),
            c["result"]
        );
    }
    for c in cases["ages"].as_array().unwrap() {
        assert_eq!(format_age(c["value"].as_u64().unwrap(), now), c["result"]);
    }
    for c in cases["ageColors"].as_array().unwrap() {
        assert_eq!(age_color(c["value"].as_u64().unwrap(), now), c["result"]);
    }
    for c in cases["gaps"].as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(not_found_by_scan(
                c["total"].as_u64().unwrap(),
                c["free"].as_u64().unwrap(),
                c["scanned"].as_u64().unwrap()
            ))
            .unwrap(),
            c["result"]
        );
    }
    for c in cases["roots"].as_array().unwrap() {
        assert_eq!(
            is_drive_root(c["path"].as_str().unwrap(), c["mount"].as_str().unwrap()),
            c["result"]
        );
    }
    for c in cases["stale"].as_array().unwrap() {
        let saved = serde_json::from_value(c["saved"].clone()).unwrap();
        let drive: Option<sprawl_core::backend::Drive> = if c["drive"].is_null() {
            None
        } else {
            Some(sprawl_core::backend::Drive {
                mount: c["drive"]["mount"].as_str().unwrap().into(),
                label: "fixture".into(),
                total: c["drive"]["total"].as_u64().unwrap(),
                free: c["drive"]["free"].as_u64().unwrap(),
            })
        };
        let (old, delta) = staleness(&saved, drive.as_ref(), now);
        assert_eq!(old, c["result"]["old"]);
        assert_eq!(
            serde_json::to_value(delta.map(|n| n as i64)).unwrap(),
            c["result"]["freeDelta"]
        );
    }
}
#[test]
fn linked_hover_and_tile_dimming() {
    use crate::state::{linked_hover, tile_dim};
    let tree:Node=serde_json::from_value(serde_json::json!({"id":0,"name":"root","kind":"dir","size":100,"children":[{"id":1,"name":"folder","kind":"dir","size":100,"children":[{"id":2,"name":"file","kind":"file","size":100}]}]})).unwrap();
    assert_eq!(linked_hover(&tree, Some(2)), Some(1));
    assert_eq!(linked_hover(&tree, Some(1)), Some(1));
    assert_eq!(linked_hover(&tree, Some(0)), Some(0));
    assert_eq!(linked_hover(&tree, Some(999)), Some(999));
    assert_eq!(linked_hover(&tree, None), None);
    let lit = [Some(1), Some(2)].into_iter().collect();
    assert!(!tile_dim(true, None, Some("code"), Some(1), Some(&lit)));
    assert!(tile_dim(
        false,
        Some("video"),
        Some("code"),
        Some(2),
        Some(&lit)
    ));
    assert!(tile_dim(true, None, None, Some(3), Some(&lit)));
    assert!(!tile_dim(
        false,
        Some("code"),
        Some("code"),
        Some(2),
        Some(&lit)
    ));
    assert!(!tile_dim(false, None, None, None, None));
    assert!(tile_dim(false, None, None, None, Some(&lit)));
}
