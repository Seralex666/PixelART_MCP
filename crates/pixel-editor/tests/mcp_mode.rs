//! `pixelart-studio --mcp`: dieselbe Datei wie der Editor muss als MCP-Server
//! über stdin/stdout sprechen, so wie Claude sie startet. Unter Windows ist das
//! der heikle Fall, weil das Programm dort kein Konsolenfenster hat.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const STUDIO: &str = env!("CARGO_BIN_EXE_pixelart-studio");

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(home: &PathBuf) -> Self {
        let mut child = Command::new(STUDIO)
            .arg("--mcp")
            .arg("--home")
            .arg(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the studio starts in MCP mode");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            next_id: 0,
        }
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
            let read = self.stdout.read_line(&mut line).unwrap();
            assert!(read > 0, "the server closed the connection");
            let msg: Value = serde_json::from_str(&line).expect("every line is JSON");
            if msg["id"] == id {
                return msg;
            }
        }
    }

    /// Ruft ein Werkzeug auf und gibt seinen Text zurück.
    fn call(&mut self, tool: &str, arguments: Value) -> String {
        let reply = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        let result = &reply["result"];
        assert_ne!(result["isError"], true, "{tool}: {result}");
        result["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_studio_program_is_also_the_mcp_server() {
    let home = std::env::temp_dir().join(format!("pixelart-studio-mcp-{}", uuid::Uuid::new_v4()));
    let mut c = Client::start(&home);
    let init = c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "pixelart-studio");
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for needed in ["get_studio_info", "create_character", "draw", "export"] {
        assert!(names.contains(&needed), "{needed} is missing in {names:?}");
    }

    // Einmal durch: Projekt anlegen, zeichnen, exportieren.
    c.call("create_project", json!({"project": "Probe"}));
    c.call(
        "create_image",
        json!({"project": "Probe", "name": "Punkt", "width": 4, "height": 4}),
    );
    c.call(
        "draw",
        json!({
            "project": "Probe", "asset": "Punkt",
            "ops": [{"op": "pixels", "color": "#ff0000", "points": [[1, 2]]}],
        }),
    );
    let exported: Value = serde_json::from_str(&c.call(
        "export",
        json!({"project": "Probe", "asset": "Punkt", "format": "png"}),
    ))
    .unwrap();
    let file = PathBuf::from(exported["files"][0].as_str().unwrap());
    assert_eq!(file, home.join("exports").join("Probe").join("Punkt.png"));
    assert!(std::fs::metadata(&file).unwrap().len() > 0);
    // Der Editor findet dasselbe Projekt im selben Ordner.
    assert!(home.join("Probe.pxproj").is_file());
}

#[test]
fn version_goes_to_standard_output() {
    let out = Command::new(STUDIO).arg("--version").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("pixelart-studio "), "{text:?}");
}
