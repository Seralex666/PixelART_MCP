//! Startet den echten Server und spricht mit ihm MCP über stdin/stdout,
//! genau wie Claude Desktop oder Claude Code es tun.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    home: PathBuf,
}

struct ToolResult {
    is_error: bool,
    text: String,
    image: Option<Vec<u8>>,
}

impl Client {
    fn start() -> Self {
        let home = std::env::temp_dir().join(format!("pixelart-mcp-test-{}", uuid::Uuid::new_v4()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_pixelart-mcp"))
            .arg("--home")
            .arg(&home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("server starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin,
            stdout,
            next_id: 0,
            home,
        };
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "pixelart-studio");
        assert!(init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("REAL pixel art"));
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server closed the connection"
            );
            let msg: Value =
                serde_json::from_str(&line).expect("server writes only JSON to stdout");
            if msg["id"] == id {
                return msg;
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let result = &r["result"];
        assert!(result.is_object(), "protocol error calling {tool}: {r}");
        let mut out = ToolResult {
            is_error: result["isError"].as_bool().unwrap_or(false),
            text: String::new(),
            image: None,
        };
        for c in result["content"].as_array().unwrap() {
            match c["type"].as_str().unwrap() {
                "text" => out.text.push_str(c["text"].as_str().unwrap()),
                "image" => {
                    use base64::Engine;
                    assert_eq!(c["mimeType"], "image/png");
                    let data = base64::engine::general_purpose::STANDARD
                        .decode(c["data"].as_str().unwrap())
                        .unwrap();
                    out.image = Some(data);
                }
                other => panic!("unexpected content type {other}"),
            }
        }
        out
    }

    fn ok(&mut self, tool: &str, args: Value) -> ToolResult {
        let r = self.call(tool, args);
        assert!(!r.is_error, "{tool} failed: {}", r.text);
        r
    }

    fn err(&mut self, tool: &str, args: Value) -> String {
        let r = self.call(tool, args);
        assert!(
            r.is_error,
            "{tool} should have failed but returned: {}",
            r.text
        );
        r.text
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn legend() -> Value {
    json!({"k": "#1a1c2c", "s": "#94b0c2", "r": "#b13e53"})
}

#[test]
fn lists_all_tools_with_schemas() {
    let mut c = Client::start();
    let r = c.request("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "get_studio_info",
        "create_project",
        "get_project",
        "create_image",
        "create_character",
        "create_animation",
        "delete_asset",
        "copy_character",
        "draw",
        "view",
        "get_pixels",
        "edit_layers",
        "edit_frames",
        "set_palette",
        "add_character_colors",
        "export",
        "import_image",
        "undo",
        "set_style",
        "palette_from_image",
        "reduce_colors",
    ] {
        assert!(names.contains(&expected), "tool {expected} is missing");
    }
    for t in tools {
        assert_eq!(
            t["inputSchema"]["type"], "object",
            "{} has no object schema",
            t["name"]
        );
        assert!(!t["description"].as_str().unwrap().is_empty());
    }
    // Jede Zeichen-Op steht mit Beschreibung und Beispiel im Schema von draw.
    let draw = tools.iter().find(|t| t["name"] == "draw").unwrap();
    let schema = draw["inputSchema"].to_string();
    for op in [
        "outline", "shade", "dither", "gradient", "noise", "shader", "stamp", "grid", "flip_v",
    ] {
        assert!(
            schema.contains(&format!("\"{op}\"")),
            "op {op} is not in the draw schema"
        );
        assert!(
            ["grid", "flip_v"].contains(&op)
                || schema.contains(&format!("\\\"op\\\": \\\"{op}\\\"")),
            "op {op} has no example in the draw schema"
        );
    }
    assert!(schema.contains("per_frame") && schema.contains("light_dir"));
}

#[test]
fn character_workflow_keeps_the_character_consistent() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "Spiel"}));
    c.ok(
        "create_character",
        json!({
            "project": "Spiel", "name": "Ritter", "width": 8, "height": 8, "color_depth_bits": 8,
            "palette": ["#1a1c2c", "#94b0c2", "#b13e53"], "layers": ["beine", "kopf"],
        }),
    );
    let rows = json!(["..rr....", ".kkkk...", ".kssk...", ".kkkk..."]);
    let r = c.ok(
        "draw",
        json!({
            "project": "Spiel", "asset": "Ritter", "layer": "kopf",
            "ops": [{"op": "grid", "rows": rows, "legend": legend()}],
        }),
    );
    let png = r.image.expect("draw returns a preview");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

    // Fremde Farben rasten auf die gesperrte Palette ein und das wird gemeldet.
    let r = c.ok("draw", json!({
        "project": "Spiel", "asset": "Ritter", "layer": 0,
        "ops": [{"op": "rect", "color": "#ff0000", "x": 2, "y": 4, "w": 2, "h": 4, "filled": true}],
    }));
    assert!(r.text.contains("#ff0000→#b13e53"), "{}", r.text);

    // Gelesene Pixel entsprechen genau dem Gezeichneten.
    let px: Value = serde_json::from_str(
        &c.ok("get_pixels", json!({"project": "Spiel", "asset": "Ritter"}))
            .text,
    )
    .unwrap();
    let got: Vec<&str> = px["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(got[0], "..aa....");
    assert_eq!(px["legend"]["a"], "#b13e53");
    assert_eq!(got[4], "..aa....");

    // Animation: alle Frames starten als Kopie der Grundpose.
    c.ok(
        "create_animation",
        json!({"project": "Spiel", "character": "Ritter", "kind": "walk", "frames": 3}),
    );
    for f in 0..3 {
        let a: Value = serde_json::from_str(
            &c.ok(
                "get_pixels",
                json!({"project": "Spiel", "asset": "Ritter walk", "frame": f}),
            )
            .text,
        )
        .unwrap();
        assert_eq!(
            a["rows"], px["rows"],
            "frame {f} differs from the base pose"
        );
    }
    let r = c.ok(
        "draw",
        json!({
            "project": "Spiel", "asset": "Ritter walk", "frame": 1, "layer": "kopf",
            "ops": [{"op": "shift", "dx": 0, "dy": -1}],
        }),
    );
    assert!(
        r.text.contains("off the canvas"),
        "losing the plume must be reported: {}",
        r.text
    );

    // Palette der Figur ist fest, lässt sich aber erweitern.
    assert!(c
        .err(
            "set_palette",
            json!({"project": "Spiel", "asset": "Ritter", "palette_preset": "gray4"})
        )
        .contains("fixed"));
    c.ok(
        "add_character_colors",
        json!({"project": "Spiel", "character": "Ritter", "colors": ["#38b764"]}),
    );
    let r = c.ok(
        "draw",
        json!({
            "project": "Spiel", "asset": "Ritter walk", "frame": 2, "layer": 0,
            "ops": [{"op": "pixels", "color": "#38b764", "points": [[0, 7]]}],
        }),
    );
    assert!(!r.text.contains("adjusted"), "{}", r.text);

    let proj: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "Spiel"})).text).unwrap();
    assert_eq!(proj["consistency_issues"], json!([]));
    assert_eq!(proj["characters"][0]["animations"][0]["frames"], 3);

    // In ein zweites Projekt übernehmen.
    c.ok("create_project", json!({"project": "Spiel 2"}));
    c.ok(
        "copy_character",
        json!({"from_project": "Spiel", "character": "Ritter", "to_project": "Spiel 2"}),
    );
    let a = c
        .ok(
            "get_pixels",
            json!({"project": "Spiel", "asset": "Ritter walk", "frame": 1}),
        )
        .text;
    let b = c
        .ok(
            "get_pixels",
            json!({"project": "Spiel 2", "asset": "Ritter walk", "frame": 1}),
        )
        .text;
    assert_eq!(a, b);
}

#[test]
fn export_writes_files_inside_the_workspace() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "Export"}));
    c.ok(
        "create_image",
        json!({"project": "Export", "name": "Tile", "template": "tile-16", "color_depth_bits": 16}),
    );
    c.ok("draw", json!({
        "project": "Export", "asset": "Tile",
        "ops": [{"op": "rect", "color": "#38b764", "x": 0, "y": 0, "w": 16, "h": 16, "filled": true}],
    }));
    c.ok(
        "edit_frames",
        json!({"project": "Export", "asset": "Tile", "action": "duplicate", "frame": 0}),
    );

    for (format, count) in [
        ("png", 2),
        ("jpg", 2),
        ("sheet", 2),
        ("png8", 2),
        ("gif", 1),
    ] {
        let r: Value = serde_json::from_str(
            &c.ok(
                "export",
                json!({"project": "Export", "asset": "Tile", "format": format, "scale": 4}),
            )
            .text,
        )
        .unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), count, "{format}");
        for f in files {
            let path = PathBuf::from(f.as_str().unwrap());
            assert!(
                path.starts_with(c.home.join("exports")),
                "{} is outside the workspace",
                path.display()
            );
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
        }
        if format == "sheet" {
            assert_eq!(r["sheet"]["image_width"], 128);
            assert_eq!(r["sheet"]["frames"].as_array().unwrap().len(), 2);
        }
        if format == "gif" {
            assert_eq!(r["gif"], json!({"colors": 1, "exact": true}));
            assert!(r.get("note").is_none(), "exact colors need no warning");
        }
    }
    assert!(c
        .err(
            "export",
            json!({"project": "Export", "asset": "Tile", "format": "bmp"})
        )
        .contains("png, png8, jpg, sheet, gif or webm"));
    assert!(c
        .err(
            "export",
            json!({"project": "Export", "asset": "Tile", "format": "gif", "frame": 0})
        )
        .contains("omit `frame`"));
    assert!(c
        .err(
            "export",
            json!({"project": "Export", "asset": "Tile", "format": "webm", "scale": 3})
        )
        .contains("even scale"));
    // WebM needs ffmpeg: with it a video comes out, without it a hint how to get it.
    let webm = json!({"project": "Export", "asset": "Tile", "format": "webm", "scale": 4});
    if pixel_core::video::find_ffmpeg().is_some() {
        let r: Value = serde_json::from_str(&c.ok("export", webm).text).unwrap();
        let path = PathBuf::from(r["files"][0].as_str().unwrap());
        assert_eq!(path, c.home.join("exports/Export/Tile@4x.webm"));
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        assert!(r["note"].as_str().unwrap().contains("exact pixels"));
    } else {
        assert!(c.err("export", webm).contains("winget install"));
    }
}

#[test]
fn bad_input_gives_helpful_errors_and_saves_nothing() {
    let mut c = Client::start();
    assert!(c
        .err("get_project", json!({"project": "Fehlt"}))
        .contains("does not exist"));
    for name in ["../evil", "a/b", "", "C:\\x", "x.pxproj"] {
        c.err("create_project", json!({"project": name}));
    }
    assert_eq!(
        std::fs::read_dir(&c.home).unwrap().count(),
        0,
        "invalid names must not create files"
    );

    c.ok("create_project", json!({"project": "P"}));
    c.err("create_project", json!({"project": "P"}));
    c.err(
        "create_image",
        json!({"project": "P", "name": "Zu gross", "width": 1025, "height": 10}),
    );
    c.err(
        "create_image",
        json!({"project": "P", "name": "Tiefe", "width": 8, "height": 8, "color_depth_bits": 24}),
    );
    c.err(
        "create_image",
        json!({"project": "P", "name": "Ohne Größe"}),
    );
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Bild", "width": 4, "height": 4}),
    );
    c.err(
        "create_image",
        json!({"project": "P", "name": "bild", "width": 4, "height": 4}),
    );

    // Ein fehlerhafter Schritt mitten in der Liste verwirft den ganzen Aufruf.
    let msg = c.err(
        "draw",
        json!({
            "project": "P", "asset": "Bild",
            "ops": [
                {"op": "rect", "color": "#ff0000", "x": 0, "y": 0, "w": 4, "h": 4, "filled": true},
                {"op": "pixels", "color": "rot", "points": [[0, 0]]},
            ],
        }),
    );
    assert!(
        msg.contains("op 1") && msg.contains("nothing was saved"),
        "{msg}"
    );
    let px: Value = serde_json::from_str(
        &c.ok("get_pixels", json!({"project": "P", "asset": "Bild"}))
            .text,
    )
    .unwrap();
    assert_eq!(px["rows"][0], "....");

    assert!(c
        .err(
            "draw",
            json!({"project": "P", "asset": "Bild", "frame": 3, "ops": [{"op": "clear"}]})
        )
        .contains("frame 3"));
    assert!(c
        .err(
            "draw",
            json!({"project": "P", "asset": "Bild", "layer": "x", "ops": [{"op": "clear"}]})
        )
        .contains("no layer"));
    assert!(c
        .err("view", json!({"project": "P", "asset": "Nichts"}))
        .contains("Available: Bild"));
    // Riesige Koordinaten dürfen den Server nicht aufhängen.
    c.err("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "line", "color": "#fff000", "from": [0, 0], "to": [9000000000000i64, 5]}]}));
    c.ok("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "shift", "dx": i64::MAX, "dy": i64::MIN}]}));
    // Der Server lebt noch.
    c.ok("get_studio_info", json!({}));
}

#[test]
fn extreme_coordinates_do_not_hang_or_crash_the_server() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Bild", "width": 4, "height": 4}),
    );
    for v in [i64::MIN, i64::MAX] {
        c.err("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "line", "color": "#ff0000", "from": [v, 0], "to": [0, 0]}]}));
        c.err("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "rect", "color": "#ff0000", "x": v, "y": 0, "w": 2, "h": 2}]}));
        c.err("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "ellipse", "color": "#ff0000", "x": 0, "y": v, "w": 2, "h": 2}]}));
    }
    c.ok("get_studio_info", json!({}));
}

#[test]
fn pixels_read_as_text_can_be_edited_and_written_back() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Bild", "width": 3, "height": 1}),
    );
    c.ok("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "rect", "color": "#ff0000", "x": 0, "y": 0, "w": 3, "h": 1, "filled": true}]}));
    let px: Value = serde_json::from_str(
        &c.ok("get_pixels", json!({"project": "P", "asset": "Bild"}))
            .text,
    )
    .unwrap();
    assert_eq!(px["rows"], json!(["aaa"]));
    assert_eq!(px["legend"]["."], "transparent");
    // Mittleren Pixel im Text löschen und mit der gelieferten Legende zurückschreiben.
    c.ok("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "grid", "rows": ["a.a"], "legend": px["legend"]}]}));
    let px: Value = serde_json::from_str(
        &c.ok("get_pixels", json!({"project": "P", "asset": "Bild"}))
            .text,
    )
    .unwrap();
    assert_eq!(px["rows"], json!(["a.a"]));
    // Ohne Eintrag für '.' bleibt der Pixel dagegen unverändert.
    c.ok("draw", json!({"project": "P", "asset": "Bild", "ops": [{"op": "grid", "rows": ["..."], "legend": {"x": "#00ff00"}}]}));
    let px: Value = serde_json::from_str(
        &c.ok("get_pixels", json!({"project": "P", "asset": "Bild"}))
            .text,
    )
    .unwrap();
    assert_eq!(px["rows"], json!(["a.a"]));
}

#[test]
fn names_that_would_collide_are_rejected() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "A"}));
    c.ok(
        "create_image",
        json!({"project": "A", "name": "Tile A", "width": 2, "height": 2}),
    );
    // Gleicher Dateiname beim Export: nicht erlaubt.
    assert!(c
        .err(
            "create_image",
            json!({"project": "A", "name": "tile_a", "width": 2, "height": 2})
        )
        .contains("too similar"));
    c.err("create_project", json!({"project": "NUL"}));

    // Kopieren: Kollision mit einem fremden Asset wird auch beim Aktualisieren erkannt.
    c.ok(
        "create_character",
        json!({"project": "A", "name": "Held", "width": 4, "height": 4}),
    );
    c.ok("create_project", json!({"project": "B"}));
    c.ok(
        "copy_character",
        json!({"from_project": "A", "character": "Held", "to_project": "B"}),
    );
    c.ok(
        "create_image",
        json!({"project": "B", "name": "Held walk", "width": 2, "height": 2}),
    );
    c.ok(
        "create_animation",
        json!({"project": "A", "character": "Held", "kind": "walk"}),
    );
    let msg = c.err(
        "copy_character",
        json!({"from_project": "A", "character": "Held", "to_project": "B"}),
    );
    assert!(msg.contains("Held walk"), "{msg}");
    // Nach dem Aufräumen klappt die Aktualisierung, und nichts ist doppelt.
    c.ok(
        "delete_asset",
        json!({"project": "B", "asset": "Held walk"}),
    );
    c.ok(
        "copy_character",
        json!({"from_project": "A", "character": "Held", "to_project": "B"}),
    );
    c.ok(
        "copy_character",
        json!({"from_project": "A", "character": "Held", "to_project": "B"}),
    );
    let proj: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "B"})).text).unwrap();
    assert_eq!(proj["characters"].as_array().unwrap().len(), 1);
    assert_eq!(
        proj["characters"][0]["animations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn layer_settings_survive_new_frames() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Bild", "width": 2, "height": 2}),
    );
    c.ok(
        "edit_layers",
        json!({"project": "P", "asset": "Bild", "action": "set", "layer": 0, "visible": false}),
    );
    c.ok(
        "edit_frames",
        json!({"project": "P", "asset": "Bild", "action": "add"}),
    );
    c.ok("draw", json!({"project": "P", "asset": "Bild", "frame": 1, "ops": [{"op": "pixels", "color": "#ff0000", "points": [[0, 0]]}]}));
    // Die Ebene ist auch im neuen Frame unsichtbar: das Gesamtbild bleibt leer.
    let px: Value = serde_json::from_str(
        &c.ok(
            "get_pixels",
            json!({"project": "P", "asset": "Bild", "frame": 1}),
        )
        .text,
    )
    .unwrap();
    assert_eq!(px["rows"], json!(["..", ".."]));
}

#[test]
fn import_brings_files_back_as_image_character_and_animation() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "Import"}));
    // Eine kleine Figur zeichnen und als vergrößertes Bild und als Sheet exportieren.
    c.ok(
        "create_character",
        json!({"project": "Import", "name": "Vorlage", "width": 4, "height": 4,
               "palette": ["#111111", "#dd3333", "#33dd33"]}),
    );
    c.ok(
        "draw",
        json!({
            "project": "Import", "asset": "Vorlage",
            "ops": [{"op": "grid", "x": 0, "y": 0, "rows": ["ab..", "bc..", "..a.", "...c"],
                     "legend": {"a": "#111111", "b": "#dd3333", "c": "#33dd33"}}],
        }),
    );
    c.ok(
        "create_animation",
        json!({"project": "Import", "character": "Vorlage", "kind": "walk", "frames": 3}),
    );
    c.ok(
        "draw",
        json!({
            "project": "Import", "asset": "Vorlage walk", "frame": 1,
            "ops": [{"op": "pixels", "color": "#33dd33", "points": [[3, 0]]}],
        }),
    );
    let file = |c: &mut Client, asset: &str, format: &str, scale: u32| -> String {
        let r: Value = serde_json::from_str(
            &c.ok(
                "export",
                json!({"project": "Import", "asset": asset, "format": format, "scale": scale}),
            )
            .text,
        )
        .unwrap();
        r["files"][0].as_str().unwrap().to_string()
    };
    let big = file(&mut c, "Vorlage", "png", 8);
    let sheet = file(&mut c, "Vorlage walk", "sheet", 4);
    let pixels = |c: &mut Client, asset: &str, frame: u32| {
        c.ok(
            "get_pixels",
            json!({"project": "Import", "asset": asset, "frame": frame}),
        )
        .text
    };
    let rows = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.len() == 4 && l.chars().all(|ch| ch == '.' || ch.is_ascii_alphanumeric()))
            .map(String::from)
            .collect()
    };

    // Vergrößertes PNG als freies Bild: kommt in Originalgröße zurück.
    let r = c.ok(
        "import_image",
        json!({"project": "Import", "path": big, "name": "Kopie"}),
    );
    assert!(
        r.text.contains("as image 'Kopie'") && r.text.contains("4×4 px"),
        "{}",
        r.text
    );
    assert!(r.text.contains("scaled down 8×"), "{}", r.text);
    assert!(r.image.is_some(), "the import shows a preview");
    assert_eq!(
        rows(&pixels(&mut c, "Kopie", 0)),
        rows(&pixels(&mut c, "Vorlage", 0))
    );

    // Sheet als neue Figur samt Animation.
    let r = c.ok(
        "import_image",
        json!({"project": "Import", "path": sheet, "as": "character", "name": "Held",
               "columns": 3, "animation": "run"}),
    );
    assert!(r.text.contains("animation 'Held run'"), "{}", r.text);
    assert_eq!(
        rows(&pixels(&mut c, "Held run", 1)),
        rows(&pixels(&mut c, "Vorlage walk", 1))
    );

    // Sheet als Animation der vorhandenen Figur: Frames werden von selbst
    // in der Größe der Figur geschnitten.
    let r = c.ok(
        "import_image",
        json!({"project": "Import", "path": sheet, "as": "animation",
               "character": "Vorlage", "animation": "attack"}),
    );
    assert!(
        r.text.contains("as animation 'Vorlage attack': 3 frame(s)"),
        "{}",
        r.text
    );
    let project: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "Import"})).text).unwrap();
    assert_eq!(project["consistency_issues"], json!([]), "{project}");

    // Fehler nennen den Grund.
    assert!(c
        .err(
            "import_image",
            json!({"project": "Import", "path": big, "name": "Kopie"})
        )
        .contains("already exists"));
    assert!(c
        .err(
            "import_image",
            json!({"project": "Import", "path": "/gibt/es/nicht.png"})
        )
        .contains("cannot import"));
    c.ok(
        "create_image",
        json!({"project": "Import", "name": "Schief", "width": 5, "height": 5}),
    );
    c.ok(
        "draw",
        json!({
            "project": "Import", "asset": "Schief",
            "ops": [{"op": "pixels", "color": "#dd3333", "points": [[0, 0], [2, 3]]}],
        }),
    );
    let crooked = file(&mut c, "Schief", "png", 1);
    assert!(c
        .err(
            "import_image",
            json!({"project": "Import", "path": crooked, "as": "animation", "character": "Held"}),
        )
        .contains("not a whole number"));
}

/// Die Farben einer Ebene als Zeilen, je Pixel ein Farbwert ("." = durchsichtig).
fn colors(c: &mut Client, asset: &str, frame: usize, layer: usize) -> Vec<Vec<String>> {
    let px: Value = serde_json::from_str(
        &c.ok(
            "get_pixels",
            json!({"project": "P", "asset": asset, "frame": frame, "layer": layer}),
        )
        .text,
    )
    .unwrap();
    px["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.as_str()
                .unwrap()
                .chars()
                .map(|ch| match px["legend"][ch.to_string()].as_str().unwrap() {
                    "transparent" => ".".to_string(),
                    color => color.to_string(),
                })
                .collect()
        })
        .collect()
}

/// Dasselbe kompakt: jede Farbe wird über `key` zu einem Buchstaben.
fn picture(
    c: &mut Client,
    asset: &str,
    frame: usize,
    layer: usize,
    key: &[(&str, char)],
) -> Vec<String> {
    colors(c, asset, frame, layer)
        .iter()
        .map(|row| {
            row.iter()
                .map(|color| match key.iter().find(|(k, _)| k == color) {
                    Some((_, ch)) => *ch,
                    None if color == "." => '.',
                    None => panic!("unexpected color {color}"),
                })
                .collect()
        })
        .collect()
}

fn image_size(png: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(png).unwrap();
    (img.width(), img.height())
}

#[test]
fn one_call_sets_up_an_image_and_draws_on_several_frames() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    let made = c.ok(
        "create_image",
        json!({"project": "P", "name": "Block", "width": 4, "height": 2, "layers": ["stein", "glut"], "frames": 4}),
    );
    assert!(
        made.text.contains("stein, glut") && made.text.contains("Frames: 4"),
        "{}",
        made.text
    );

    // Ein Aufruf zeichnet auf alle vier Frames; per_frame ändert Farbe oder verschiebt.
    let red = "#ff0000";
    let drawn = c.ok(
        "draw",
        json!({"project": "P", "asset": "Block", "frame": "all", "layer": "glut",
            "ops": [{"op": "rect", "color": red, "x": 0, "y": 0, "w": 2, "h": 2, "filled": true}],
            "per_frame": [
                {},
                {"colors": {"#ff0000": "#00ff00"}},
                {"ops": [{"op": "shift", "dx": 2, "dy": 0}]},
                {"frame": 3, "colors": {"#FF0000": "#0000ff"}, "ops": [{"op": "pixels", "color": red, "points": [[3, 1]]}]},
            ]}),
    );
    assert!(drawn.text.contains("frames 0, 1, 2, 3"), "{}", drawn.text);
    let key = [("#ff0000", 'r'), ("#00ff00", 'g'), ("#0000ff", 'b')];
    assert_eq!(picture(&mut c, "Block", 0, 1, &key), ["rr..", "rr.."]);
    assert_eq!(picture(&mut c, "Block", 1, 1, &key), ["gg..", "gg.."]);
    assert_eq!(picture(&mut c, "Block", 2, 1, &key), ["..rr", "..rr"]);
    assert_eq!(picture(&mut c, "Block", 3, 1, &key), ["bb..", "bb.r"]);
    assert_eq!(picture(&mut c, "Block", 0, 0, &key), ["....", "...."]);
    // Die Vorschau zeigt alle vier Frames nebeneinander.
    let (w, h) = image_size(drawn.image.as_ref().unwrap());
    assert!(w > 6 * h, "preview is {w}×{h}");

    // Eine Liste wählt einzelne Frames.
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Block", "frame": [0, 2], "layer": 0,
            "ops": [{"op": "pixels", "color": red, "points": [[0, 0]]}]}),
    );
    assert_eq!(picture(&mut c, "Block", 2, 0, &key)[0], "r...");
    assert_eq!(picture(&mut c, "Block", 1, 0, &key)[0], "....");

    let err = c.err(
        "draw",
        json!({"project": "P", "asset": "Block", "frame": [0, 1], "per_frame": [{"frame": 3, "ops": [{"op": "clear"}]}]}),
    );
    assert!(err.contains("not among the frames"), "{err}");
    let err = c.err(
        "draw",
        json!({"project": "P", "asset": "Block", "frame": [1, 1], "ops": [{"op": "clear"}]}),
    );
    assert!(err.contains("listed twice"), "{err}");
    let err = c.err(
        "draw",
        json!({"project": "P", "asset": "Block", "frame": 9, "ops": [{"op": "clear"}]}),
    );
    assert!(err.contains("frame 9 does not exist"), "{err}");
    let err = c.err(
        "create_image",
        json!({"project": "P", "name": "Zu viel", "width": 4, "height": 4, "frames": 0}),
    );
    assert!(err.contains("1 to 256"), "{err}");
    let err = c.err(
        "create_image",
        json!({"project": "P", "name": "Doppelt", "width": 4, "height": 4, "layers": ["a", "A"]}),
    );
    assert!(err.contains("given twice"), "{err}");
}

#[test]
fn frames_are_added_and_duplicated_several_at_once() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "B", "width": 2, "height": 1}),
    );
    c.ok("draw", json!({"project": "P", "asset": "B", "ops": [{"op": "pixels", "color": "#ff0000", "points": [[0, 0]]}]}));
    let r = c.ok(
        "edit_frames",
        json!({"project": "P", "asset": "B", "action": "duplicate", "frame": 0, "count": 3}),
    );
    assert!(
        r.text.contains("frames 1 to 3") && r.text.contains("4 frame(s)"),
        "{}",
        r.text
    );
    let key = [("#ff0000", 'r'), ("#00ff00", 'g')];
    for f in 0..4 {
        assert_eq!(picture(&mut c, "B", f, 0, &key), ["r."]);
    }
    // Frame 3 grün färben und zweimal ganz nach vorn kopieren.
    c.ok("draw", json!({"project": "P", "asset": "B", "frame": 3, "ops": [{"op": "replace_color", "from": "#ff0000", "to": "#00ff00"}]}));
    c.ok("edit_frames", json!({"project": "P", "asset": "B", "action": "duplicate", "frame": 3, "count": 2, "insert_at": 0}));
    let all: Vec<String> = (0..6)
        .map(|f| picture(&mut c, "B", f, 0, &key)[0].clone())
        .collect();
    assert_eq!(all, ["g.", "g.", "r.", "r.", "r.", "g."]);
    let r = c.ok(
        "edit_frames",
        json!({"project": "P", "asset": "B", "action": "add", "count": 2}),
    );
    assert!(r.text.contains("8 frame(s)"), "{}", r.text);
    let err = c.err(
        "edit_frames",
        json!({"project": "P", "asset": "B", "action": "add", "count": 300}),
    );
    assert!(err.contains("only 248 more fit"), "{err}");
    let err = c.err(
        "edit_frames",
        json!({"project": "P", "asset": "B", "action": "duplicate", "frame": 0, "insert_at": 99}),
    );
    assert!(err.contains("past the end"), "{err}");
}

#[test]
fn technique_ops_are_pixel_exact() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    let (dark, base, light, ink) = ("#333c57", "#566c86", "#94b0c2", "#1a1c2c");
    let key = [
        (dark, 'd'),
        (base, 'b'),
        (light, 'l'),
        (ink, 'k'),
        ("#000000", '0'),
        ("#ffffff", '1'),
        ("#ff0000", 'r'),
        ("#00ff00", 'g'),
    ];

    // Kontur und Licht um ein 4×4-Quadrat in einem 6×6-Bild.
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Stein", "width": 6, "height": 6}),
    );
    let r = c.ok(
        "draw",
        json!({"project": "P", "asset": "Stein", "ops": [
            {"op": "rect", "color": base, "x": 1, "y": 1, "w": 4, "h": 4, "filled": true},
            {"op": "shade", "light_dir": "top-left", "ramp": [dark, base, light]},
            {"op": "outline", "color": ink},
        ]}),
    );
    assert!(!r.text.contains("Warning"), "{}", r.text);
    assert_eq!(
        picture(&mut c, "Stein", 0, 0, &key),
        [".kkkk.", "kllllk", "klbbdk", "klbbdk", "kldddk", ".kkkk."]
    );
    // Mit Eck-Nachbarn schließt die Kontur auch die Ecken; eine Maske trifft nur eine Farbe.
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Stein", "ops": [
            {"op": "replace_color", "from": ink, "to": "transparent"},
            {"op": "outline", "color": ink, "diagonals": true},
            {"op": "noise", "colors": ["#ff0000"], "density": 1.0, "mask": base},
        ]}),
    );
    assert_eq!(
        picture(&mut c, "Stein", 0, 0, &key),
        ["kkkkkk", "kllllk", "klrrdk", "klrrdk", "kldddk", "kkkkkk"]
    );

    // Verlauf in harten Stufen, linear und rund.
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Verlauf", "width": 4, "height": 1}),
    );
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Verlauf", "ops": [
        {"op": "gradient", "colors": ["#000000", "#ffffff"], "direction": "right"}]}),
    );
    assert_eq!(picture(&mut c, "Verlauf", 0, 0, &key), ["0011"]);
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Verlauf", "ops": [
        {"op": "gradient", "colors": ["#ffffff", "#000000"], "center": [0, 0], "radius": 2}]}),
    );
    assert_eq!(picture(&mut c, "Verlauf", 0, 0, &key), ["1000"]);
    // Raster: nur die zwei Farben, links mehr von der ersten, und jedes Mal dasselbe Bild.
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Raster", "width": 8, "height": 8}),
    );
    let dither = json!({"project": "P", "asset": "Raster", "ops": [
        {"op": "dither", "from_color": "#000000", "to_color": "#ffffff", "direction": "right", "pattern": "bayer4"}]});
    c.ok("draw", dither.clone());
    let first = picture(&mut c, "Raster", 0, 0, &key);
    c.ok("draw", dither);
    assert_eq!(first, picture(&mut c, "Raster", 0, 0, &key));
    let whites = |rows: &[String], cols: std::ops::Range<usize>| -> usize {
        rows.iter()
            .map(|r| r[cols.clone()].matches('1').count())
            .sum()
    };
    assert_eq!(whites(&first, 0..1), 0);
    assert_eq!(whites(&first, 7..8), 8);
    assert!(whites(&first, 1..4) < whites(&first, 4..7), "{first:?}");

    // Körnung: gleicher Startwert, gleiches Bild; anderer Startwert, anderes Bild.
    let noise = |seed: u64| {
        json!({"project": "P", "asset": "Raster", "ops": [
        {"op": "clear"},
        {"op": "noise", "colors": ["#ff0000", "#00ff00"], "density": 0.5, "seed": seed}]})
    };
    c.ok("draw", noise(1));
    let a = picture(&mut c, "Raster", 0, 0, &key);
    c.ok("draw", noise(2));
    let b = picture(&mut c, "Raster", 0, 0, &key);
    c.ok("draw", noise(1));
    assert_eq!(a, picture(&mut c, "Raster", 0, 0, &key));
    assert_ne!(a, b);
    let hit: usize = a
        .iter()
        .map(|r| r.chars().filter(|ch| *ch != '.').count())
        .sum();
    assert!((16..=48).contains(&hit), "density 0.5 hit {hit} of 64");

    // Formel je Pixel: Index in die Farbliste, negativ lässt den Pixel stehen, Frames zählen mit.
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Formel", "width": 4, "height": 1, "frames": 2}),
    );
    c.ok("draw", json!({"project": "P", "asset": "Formel", "frame": "all", "ops": [
        {"op": "shader", "formula": "x == 3 ? -1 : x + frame", "colors": ["#000000", "#ff0000", "#ffffff"]}]}));
    assert_eq!(picture(&mut c, "Formel", 0, 0, &key), ["0r1."]);
    assert_eq!(picture(&mut c, "Formel", 1, 0, &key), ["r11."]);
    let err = c.err(
        "draw",
        json!({"project": "P", "asset": "Formel", "ops": [
        {"op": "shader", "formula": "system(1)", "colors": ["#000000"]}]}),
    );
    assert!(
        err.contains("unknown function 'system'") && err.contains("nothing was saved"),
        "{err}"
    );

    // Stempel: ein Stück kopieren, spiegeln und umfärben.
    c.ok("draw", json!({"project": "P", "asset": "Verlauf", "ops": [
        {"op": "clear"},
        {"op": "grid", "rows": ["0r"], "legend": {"0": "#000000", "r": "#ff0000"}},
        {"op": "stamp", "src": [0, 0, 2, 1], "x": 2, "y": 0, "flip_h": true, "color_map": {"#000000": "#ffffff"}}]}));
    assert_eq!(picture(&mut c, "Verlauf", 0, 0, &key), ["0rr1"]);
    // Aus einem anderen Asset, gedreht: aus der Zeile wird eine Spalte.
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Ziel", "width": 2, "height": 4}),
    );
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Ziel", "ops": [
        {"op": "stamp", "from_asset": "Verlauf", "x": 1, "y": 0, "rotate": 90}]}),
    );
    assert_eq!(
        picture(&mut c, "Ziel", 0, 0, &key),
        [".0", ".r", ".r", ".1"]
    );

    // Hinweise statt stiller Fehlschläge.
    let r = c.ok(
        "draw",
        json!({"project": "P", "asset": "Ziel", "ops": [
        {"op": "shade", "light_dir": "top", "ramp": [dark, light], "mask": "#123456"}]}),
    );
    assert!(
        r.text
            .contains("no pixel in the area matches mask '#123456'"),
        "{}",
        r.text
    );
    for (op, needle) in [
        (
            json!({"op": "shade", "light_dir": "north", "ramp": [dark, light]}),
            "unknown light_dir",
        ),
        (
            json!({"op": "shade", "light_dir": "top", "ramp": [dark]}),
            "at least 2 colors",
        ),
        (
            json!({"op": "dither", "from_color": dark, "to_color": light, "pattern": "dots"}),
            "unknown pattern",
        ),
        (
            json!({"op": "noise", "colors": [dark], "density": 2}),
            "between 0 and 1",
        ),
        (
            json!({"op": "stamp", "x": 0, "y": 0, "rotate": 45}),
            "0, 90, 180 or 270",
        ),
        (
            json!({"op": "gradient", "colors": [dark, light], "mask": "bunt"}),
            "invalid mask",
        ),
        (
            json!({"op": "stamp", "x": 0, "y": 0, "from_asset": "Fehlt"}),
            "Fehlt",
        ),
    ] {
        let err = c.err(
            "draw",
            json!({"project": "P", "asset": "Ziel", "ops": [op]}),
        );
        assert!(err.contains(needle), "expected '{needle}' in: {err}");
    }
}

#[test]
fn technique_ops_respect_a_locked_palette() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_character",
        json!({"project": "P", "name": "Held", "width": 8, "height": 8, "palette": ["#000000", "#ffffff"]}),
    );
    // Der Verlauf mischt Grautöne; auf der Figur bleiben nur ihre zwei Farben.
    let r = c.ok(
        "draw",
        json!({"project": "P", "asset": "Held", "ops": [
        {"op": "gradient", "colors": ["#000000", "#ffffff"], "steps": 8, "direction": "right"},
        {"op": "noise", "colors": ["#ff0000"], "density": 0.3, "seed": 3}]}),
    );
    assert!(r.text.contains("#ff0000→"), "{}", r.text);
    let used: std::collections::BTreeSet<String> =
        colors(&mut c, "Held", 0, 0).into_iter().flatten().collect();
    assert_eq!(used.into_iter().collect::<Vec<_>>(), ["#000000", "#ffffff"]);
}

#[test]
fn undo_takes_back_draw_calls_but_never_foreign_changes() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "B", "width": 2, "height": 1}),
    );
    let key = [("#ff0000", 'r'), ("#00ff00", 'g')];
    let dot = |x: i64, color: &str| json!({"project": "P", "asset": "B", "ops": [{"op": "pixels", "color": color, "points": [[x, 0]]}]});
    c.ok("draw", dot(0, "#ff0000"));
    c.ok("draw", dot(1, "#00ff00"));
    assert_eq!(picture(&mut c, "B", 0, 0, &key), ["rg"]);
    let r = c.ok("undo", json!({"project": "P"}));
    assert!(
        r.text.contains("Undid 1 draw call") && r.text.contains("1 more"),
        "{}",
        r.text
    );
    assert!(r.image.is_some());
    assert_eq!(picture(&mut c, "B", 0, 0, &key), ["r."]);
    c.ok("undo", json!({"project": "P", "steps": 5}));
    assert_eq!(picture(&mut c, "B", 0, 0, &key), [".."]);
    assert!(c
        .err("undo", json!({"project": "P"}))
        .contains("nothing to undo"));

    // Hat inzwischen jemand anderes das Bild geändert, bleibt es unangetastet.
    c.ok("draw", dot(0, "#ff0000"));
    c.ok(
        "edit_layers",
        json!({"project": "P", "asset": "B", "action": "add", "name": "oben"}),
    );
    let err = c.err("undo", json!({"project": "P"}));
    assert!(err.contains("was changed after that draw call"), "{err}");
    assert_eq!(picture(&mut c, "B", 0, 0, &key), ["r."]);
}

#[test]
fn view_shows_grid_region_and_tiles() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Kachel", "width": 16, "height": 16, "frames": 2}),
    );
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Kachel", "frame": "all", "ops": [
        {"op": "rect", "color": "#ff0000", "x": 0, "y": 0, "w": 16, "h": 16, "filled": true}]}),
    );
    let plain = c.ok(
        "view",
        json!({"project": "P", "asset": "Kachel", "frame": 0}),
    );
    assert!(plain.text.contains("gray checkerboard"), "{}", plain.text);
    let plain_size = image_size(plain.image.as_ref().unwrap());
    assert_eq!(plain_size, (512, 512));

    let grid = c.ok(
        "view",
        json!({"project": "P", "asset": "Kachel", "frame": 0, "grid": true}),
    );
    assert!(
        grid.text.contains("Grid: one cell per pixel"),
        "{}",
        grid.text
    );
    // Mit Raster kommt ein Rand für die Koordinaten dazu.
    assert_eq!(
        image_size(grid.image.as_ref().unwrap()),
        (512 + 18, 512 + 8)
    );
    let img = image::load_from_memory(grid.image.as_ref().unwrap())
        .unwrap()
        .to_rgba8();
    // Linie auf der Kante von Pixel 8 (dunkel), daneben die rote Fläche.
    assert_eq!(img.get_pixel(18 + 8 * 32, 8 + 5).0, [20, 20, 28, 255]);
    assert_eq!(img.get_pixel(18 + 8 * 32 + 5, 8 + 5).0, [255, 0, 0, 255]);

    let zoom = c.ok(
        "view",
        json!({"project": "P", "asset": "Kachel", "frame": 0, "zoom_region": [4, 4, 4, 2]}),
    );
    assert!(
        zoom.text.contains("Region x 4 to 7, y 4 to 5"),
        "{}",
        zoom.text
    );
    assert_eq!(image_size(zoom.image.as_ref().unwrap()), (4 * 32, 2 * 32));

    let tiled = c.ok("view", json!({"project": "P", "asset": "Kachel", "frame": 0, "tile": true, "background": "#000000"}));
    assert!(
        tiled.text.contains("Tiled 3×3") && tiled.text.contains("Background: #000000"),
        "{}",
        tiled.text
    );
    assert_eq!(
        image_size(tiled.image.as_ref().unwrap()),
        (48 * 10, 48 * 10)
    );

    // Zwei Frames nebeneinander sind zu klein fürs Raster: das sagt die Antwort.
    let small = c.ok("view", json!({"project": "P", "asset": "Kachel", "frame": [0, 1], "grid": true, "zoom_region": [0, 0, 16, 16]}));
    assert!(!small.text.contains("left out"), "{}", small.text);
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Groß", "width": 300, "height": 300}),
    );
    let big = c.ok(
        "view",
        json!({"project": "P", "asset": "Groß", "grid": true}),
    );
    assert!(big.text.contains("The grid was left out"), "{}", big.text);
    assert!(c
        .err(
            "view",
            json!({"project": "P", "asset": "Groß", "tile": true})
        )
        .contains("up to 256×256"));
    assert!(c
        .err(
            "view",
            json!({"project": "P", "asset": "Kachel", "zoom_region": [40, 0, 4, 4]})
        )
        .contains("lies outside"));
    assert!(c
        .err(
            "view",
            json!({"project": "P", "asset": "Kachel", "background": "blau"})
        )
        .contains("invalid background"));
}

#[test]
fn studio_info_is_in_english() {
    let mut c = Client::start();
    let info = c.ok("get_studio_info", json!({})).text;
    for german in ["Kacheln", "Figur", "Ruhepose", "Gehzyklus"] {
        assert!(!info.contains(german), "found German text '{german}'");
    }
    assert!(info.contains("tiles for tile maps") && info.contains("Walk cycle"));
}

#[test]
fn project_overview_points_out_likely_slips() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    c.ok(
        "create_image",
        json!({"project": "P", "name": "Box", "width": 16, "height": 16, "frames": 2}),
    );
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Box", "frame": "all", "ops": [
            {"op": "rect", "color": "#41a6f6", "x": 4, "y": 4, "w": 8, "h": 8, "filled": true},
            {"op": "outline", "color": "#1a1c2c"}]}),
    );
    let clean: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "P"})).text).unwrap();
    assert!(clean["images"][0].get("hints").is_none(), "{clean}");

    // Ein verirrter Pixel in Frame 1 und eine Lücke im Umriss in beiden Frames.
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Box", "frame": 1, "ops": [
            {"op": "pixels", "color": "#1a1c2c", "points": [[0, 15]]}]}),
    );
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Box", "frame": "all", "ops": [
            {"op": "pixels", "color": "#41a6f6", "points": [[3, 8]]}]}),
    );
    let o: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "P"})).text).unwrap();
    let hints: Vec<&str> = o["images"][0]["hints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h.as_str().unwrap())
        .collect();
    assert!(
        hints
            .iter()
            .any(|h| h.starts_with("frame 1: 1 stray pixel") && h.contains("(0,15)")),
        "{hints:?}"
    );
    assert!(
        hints
            .iter()
            .any(|h| h.starts_with("frame 0, 1: the outline (#1a1c2c)") && h.contains("(3,8)")),
        "{hints:?}"
    );
    let view = c.ok("view", json!({"project": "P", "asset": "Box", "frame": 1}));
    assert!(
        view.text.contains("Hints:") && view.text.contains("stray pixel"),
        "{}",
        view.text
    );
}

#[test]
fn the_project_style_fills_in_palette_light_and_outline() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    assert!(c
        .err("set_style", json!({"project": "P"}))
        .contains("at least one"));
    assert!(c
        .err(
            "set_style",
            json!({"project": "P", "light_dir": "sideways"})
        )
        .contains("unknown light_dir"));
    let set = c.ok(
        "set_style",
        json!({"project": "P", "palette": ["#000000", "#555555", "#aaaaaa", "#ffffff"],
               "light_dir": "Top Left", "outline_color": "#000000", "notes": "1 px Umriss"}),
    );
    let v: Value = serde_json::from_str(&set.text).unwrap();
    assert_eq!(v["light_dir"], "top-left");
    assert_eq!(v["outline_color"], "#000000");
    let o: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "P"})).text).unwrap();
    assert_eq!(o["style"]["notes"], "1 px Umriss");
    assert_eq!(o["style"]["palette"].as_array().unwrap().len(), 4);

    // Neue Assets starten mit der Palette des Stils, außer sie nennen eine eigene.
    let made = c.ok(
        "create_character",
        json!({"project": "P", "name": "Held", "width": 6, "height": 6}),
    );
    let v: Value = serde_json::from_str(&made.text).unwrap();
    assert_eq!(v["palette"].as_array().unwrap().len(), 4);
    assert_eq!(v["palette_source"], "project style");
    let own = c.ok(
        "create_image",
        json!({"project": "P", "name": "Eigen", "width": 4, "height": 4, "palette_preset": "gray8"}),
    );
    assert!(!own.text.contains("project style"), "{}", own.text);

    // outline ohne Farbe und shade ohne Licht nehmen die Vorgaben des Stils.
    c.ok(
        "draw",
        json!({"project": "P", "asset": "Held", "ops": [
            {"op": "rect", "color": "#aaaaaa", "x": 1, "y": 1, "w": 4, "h": 4, "filled": true},
            {"op": "shade", "ramp": ["#555555", "#aaaaaa", "#ffffff"], "mask": "#aaaaaa"},
            {"op": "outline"}]}),
    );
    let key = [
        ("#000000", 'k'),
        ("#555555", 'd'),
        ("#aaaaaa", 'm'),
        ("#ffffff", 'w'),
    ];
    assert_eq!(
        picture(&mut c, "Held", 0, 0, &key),
        [".kkkk.", "kwwwwk", "kwmmdk", "kwmmdk", "kwdddk", ".kkkk."]
    );

    // Ohne Stil fehlt die Umrissfarbe.
    c.ok("set_style", json!({"project": "P", "clear": true}));
    let err = c.err(
        "draw",
        json!({"project": "P", "asset": "Held", "ops": [{"op": "outline"}]}),
    );
    assert!(err.contains("`color` is missing"), "{err}");
    let o: Value =
        serde_json::from_str(&c.ok("get_project", json!({"project": "P"})).text).unwrap();
    assert!(o.get("style").is_none());
}

#[test]
fn palettes_come_from_pictures_and_assets_shrink_to_fewer_colors() {
    let mut c = Client::start();
    c.ok("create_project", json!({"project": "P"}));
    // Ein Bild mit vier dunklen Rottönen, zwei Grüntönen und einem hellen Glanzpunkt.
    let file = c.home.join("vorlage.png");
    std::fs::create_dir_all(&c.home).unwrap();
    let mut img = image::RgbaImage::new(8, 2);
    let reds = [[100, 0, 0], [104, 0, 0], [108, 0, 0], [112, 0, 0]];
    for x in 0..8u32 {
        let red = reds[(x % 4) as usize];
        img.put_pixel(x, 0, image::Rgba([red[0], red[1], red[2], 255]));
        let g = if x < 7 { [0, 160, 0] } else { [0, 164, 0] };
        img.put_pixel(x, 1, image::Rgba([g[0], g[1], g[2], 255]));
    }
    img.put_pixel(7, 0, image::Rgba([255, 255, 255, 255]));
    img.save(&file).unwrap();

    let r = c.ok(
        "palette_from_image",
        json!({"path": file.to_string_lossy(), "max_colors": 3}),
    );
    let v: Value = serde_json::from_str(&r.text).unwrap();
    // Der seltene Glanzpunkt bleibt, die ähnlichen Töne fallen zusammen.
    let found: Vec<&str> = v["colors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(found[2], "#ffffff");
    assert!(
        found[0].starts_with("#6") && found[1] == "#00a000",
        "{found:?}"
    );
    assert_eq!(v["source_colors"], 7);
    assert!(r.image.is_some());
    assert!(c
        .err("palette_from_image", json!({"project": "P"}))
        .contains("give either `path`"));

    c.ok(
        "import_image",
        json!({"project": "P", "path": file.to_string_lossy(), "name": "Bild"}),
    );
    let red = c.ok(
        "reduce_colors",
        json!({"project": "P", "asset": "Bild", "colors": 3}),
    );
    assert!(red.text.contains("from 7 to 3 colors"), "{}", red.text);
    let after: std::collections::BTreeSet<String> = colors(&mut c, "Bild", 0, 0)
        .into_iter()
        .flatten()
        .filter(|c| c != ".")
        .collect();
    assert_eq!(after.len(), 3);
    assert!(
        after.contains("#ffffff") && after.contains("#00a000"),
        "{after:?}"
    );
    let again = c.ok(
        "reduce_colors",
        json!({"project": "P", "asset": "Bild", "colors": 8}),
    );
    assert!(again.text.contains("nothing was changed"), "{}", again.text);
    // reduce_colors lässt sich wie ein Zeichenaufruf zurücknehmen.
    c.ok("undo", json!({"project": "P"}));
    let back: std::collections::BTreeSet<String> =
        colors(&mut c, "Bild", 0, 0).into_iter().flatten().collect();
    assert_eq!(back.len(), 7);

    // Aus einem Asset liest die Palette genauso.
    let from_asset = c.ok(
        "palette_from_image",
        json!({"project": "P", "asset": "Bild", "max_colors": 16}),
    );
    let v: Value = serde_json::from_str(&from_asset.text).unwrap();
    assert_eq!(v["count"], 7);
}
