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
