//! WebM-Export über das Programm `ffmpeg`.
//!
//! WebM lässt sich nicht mit vertretbarem Aufwand selbst schreiben, deshalb
//! übernimmt das ein vorhandenes ffmpeg. Fehlt es, funktioniert alles andere
//! weiter, nur dieser eine Export nicht.

use crate::export::{self, MAX_SCALE};
use crate::{Error, Result, Sprite};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
const FFMPEG: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG: &str = "ffmpeg";

/// Feinste Zeiteinheit eines Videos in Millisekunden. Frame-Dauern werden
/// darauf gerundet, wie beim GIF.
const TICK_MS: u32 = 10;
/// Mehr Einzelbilder schreibt ein Export nicht in ein Video.
const MAX_VIDEO_FRAMES: u32 = 20_000;

/// Sucht ffmpeg: zuerst die Umgebungsvariable `PIXELART_FFMPEG`, dann neben
/// dem eigenen Programm, dann im Suchpfad des Systems und zuletzt dort, wo der
/// Windows-Paketmanager winget es ablegt.
///
/// Der letzte Ort ist nötig, weil ein laufendes Programm den Suchpfad von
/// seinem Start behält: Direkt nach der Installation stünde ffmpeg sonst erst
/// nach einem Neustart zur Verfügung, auch für einen von Claude gestarteten
/// MCP-Server.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PIXELART_FFMPEG").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(FFMPEG)))
        .filter(|p| p.is_file());
    if beside.is_some() {
        return beside;
    }
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(FFMPEG))
            .find(|p| p.is_file())
    });
    if on_path.is_some() {
        return on_path;
    }
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .and_then(|dir| find_in_winget(Path::new(&dir), FFMPEG))
}

/// Kennung des ffmpeg-Pakets bei winget.
pub const WINGET_PACKAGE: &str = "Gyan.FFmpeg";

/// Sucht ffmpeg in den Ordnern von winget unterhalb von `%LOCALAPPDATA%`.
/// winget legt Programme ohne Installer unter `Microsoft/WinGet/Packages/<Paket>_…`
/// ab und, wenn Windows es erlaubt, eine Verknüpfung unter `Microsoft/WinGet/Links`.
pub fn find_in_winget(local_app_data: &Path, file: &str) -> Option<PathBuf> {
    let winget = local_app_data.join("Microsoft").join("WinGet");
    let link = winget.join("Links").join(file);
    if link.is_file() {
        return Some(link);
    }
    let mut packages: Vec<PathBuf> = std::fs::read_dir(winget.join("Packages"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(WINGET_PACKAGE))
        })
        .collect();
    packages.sort();
    // Im Paket liegt ein Ordner je Version, darin `bin/ffmpeg.exe`.
    packages
        .iter()
        .rev()
        .find_map(|package| find_file(package, file, 3))
}

/// Sucht eine Datei in einem Ordner und höchstens `depth` Ebenen darunter.
/// Bei mehreren Treffern gewinnt der alphabetisch letzte, also die neueste Version.
fn find_file(dir: &Path, file: &str, depth: u32) -> Option<PathBuf> {
    let direct = dir.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    folders
        .iter()
        .rev()
        .find_map(|folder| find_file(folder, file, depth - 1))
}

/// Die Argumente, mit denen winget ffmpeg installiert, ohne nachzufragen.
pub const WINGET_INSTALL_ARGS: [&str; 8] = [
    "install",
    "--id",
    WINGET_PACKAGE,
    "--exact",
    "--source",
    "winget",
    "--accept-source-agreements",
    "--accept-package-agreements",
];

/// Installiert ffmpeg über den Windows-Paketmanager winget und gibt zurück, wo
/// es danach liegt. winget zeigt den Fortschritt in einem eigenen Fenster. Das
/// kann einige Minuten dauern; der Aufruf wartet so lange.
#[cfg(windows)]
pub fn install_ffmpeg() -> Result<PathBuf> {
    if let Some(found) = find_ffmpeg() {
        return Ok(found);
    }
    let status = Command::new("winget")
        .args(WINGET_INSTALL_ARGS)
        .stdin(Stdio::null())
        .status();
    // winget meldet auch dann einen Fehler, wenn das Paket schon da war.
    // Entscheidend ist, ob ffmpeg danach zu finden ist.
    if let Some(found) = find_ffmpeg() {
        return Ok(found);
    }
    Err(Error::Video(match status {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => format!(
            "Der Windows-Paketmanager winget wurde nicht gefunden. {FFMPEG_MANUAL}"
        ),
        Err(err) => format!("winget lässt sich nicht starten ({err}). {FFMPEG_MANUAL}"),
        Ok(status) if status.success() => format!(
            "winget meldet Erfolg, aber ffmpeg ist nicht zu finden. Starte das Studio neu. {FFMPEG_MANUAL}"
        ),
        Ok(status) => format!(
            "Die Installation ist fehlgeschlagen (winget: {status}). {FFMPEG_MANUAL}"
        ),
    }))
}

/// Außerhalb von Windows gibt es keinen einheitlichen Weg zur Installation.
#[cfg(not(windows))]
pub fn install_ffmpeg() -> Result<PathBuf> {
    find_ffmpeg().ok_or_else(|| {
        Error::Video(
            "Die automatische Installation gibt es nur unter Windows. Installiere ffmpeg über die Paketverwaltung deines Systems.".into(),
        )
    })
}

/// Ob [`install_ffmpeg`] auf diesem System etwas installieren kann.
pub const CAN_INSTALL_FFMPEG: bool = cfg!(windows);

/// Der Weg von Hand, falls die Installation aus dem Programm nicht klappt.
pub const FFMPEG_MANUAL: &str = "Von Hand geht es so: in der Eingabeaufforderung „winget install Gyan.FFmpeg“ ausführen, oder ffmpeg.exe in denselben Ordner wie das Studio legen.";

/// Hinweis für Menschen, wie man ffmpeg bekommt.
pub const FFMPEG_HELP: &str = "Für WebM wird das kostenlose Programm ffmpeg gebraucht. Im Studio lässt es sich im Export-Dialog bei „WebM“ mit einem Klick installieren. Von Hand: in der Eingabeaufforderung „winget install Gyan.FFmpeg“ ausführen, oder ffmpeg.exe in denselben Ordner wie das Studio legen.";

/// Wie die Frames eines Sprites auf Videobilder verteilt werden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPlan {
    /// Dauer eines Videobildes in Millisekunden.
    pub tick_ms: u32,
    /// Wie viele Videobilder jeder Frame bekommt.
    pub repeats: Vec<u32>,
}

impl VideoPlan {
    /// Videos haben eine feste Bildrate. Sie wird so gewählt, dass jede
    /// Frame-Dauer ein ganzes Vielfaches eines Videobildes ist.
    pub fn new(durations_ms: &[u32]) -> Self {
        let rounded: Vec<u32> = durations_ms
            .iter()
            .map(|&ms| ((ms + TICK_MS / 2) / TICK_MS).max(1) * TICK_MS)
            .collect();
        let tick_ms = rounded.iter().copied().fold(0, gcd).max(TICK_MS);
        let repeats = rounded.iter().map(|&ms| ms / tick_ms).collect();
        Self { tick_ms, repeats }
    }

    pub fn total_frames(&self) -> u32 {
        self.repeats.iter().sum()
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Die Argumente für ffmpeg bis auf die Zieldatei, die direkt danach folgt: rohe RGBA-Bilder von der
/// Standardeingabe, VP9 verlustfrei als Ausgabe. Mit Transparenz, falls das
/// Bild welche hat.
pub fn ffmpeg_args(width: u32, height: u32, tick_ms: u32, alpha: bool) -> Vec<String> {
    let format = if alpha { "yuva420p" } else { "yuv420p" };
    // Die Umrechnung der Farben wird fest vorgegeben und in der Datei vermerkt.
    // Ohne den Vermerk rät jedes Abspielprogramm selbst, und die Farben
    // weichen sichtbar ab.
    let filter = format!(
        "scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int+neighbor,format={format}"
    );
    let size = format!("{width}x{height}");
    let rate = format!("1000/{tick_ms}");
    let mut args = vec!["-y", "-hide_banner", "-loglevel", "error"];
    args.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size]);
    args.extend(["-r", &rate, "-i", "-", "-an"]);
    args.extend(["-c:v", "libvpx-vp9", "-lossless", "1", "-vf", &filter]);
    args.extend(["-colorspace", "bt709", "-color_primaries", "bt709"]);
    args.extend(["-color_trc", "bt709", "-color_range", "tv"]);
    // Das Format steht fest und hängt nicht am Namen der Zieldatei.
    args.extend(["-f", "webm"]);
    if alpha {
        // Ohne diese Einstellung verwirft der Encoder die Transparenz.
        args.extend(["-auto-alt-ref", "0"]);
    }
    args.into_iter().map(String::from).collect()
}

/// Schreibt alle Frames als WebM-Video (VP9, verlustfrei komprimiert).
///
/// `scale` muss gerade sein: Das Videoformat speichert Farbe nur für Blöcke
/// aus 2×2 Bildpunkten. Bei gerader Vergrößerung deckt sich jeder Block mit
/// einem Pixel, und die Kanten bleiben scharf. Die Farben können trotzdem um
/// ein bis zwei Stufen (von 255) vom Original abweichen; für exakte Pixel gibt
/// es PNG, Spritesheet und GIF.
pub fn animation_webm(s: &Sprite, scale: u32, out: &Path, ffmpeg: &Path) -> Result<()> {
    if scale == 0 || scale > MAX_SCALE || !scale.is_multiple_of(2) {
        return Err(Error::OddVideoScale);
    }
    let durations: Vec<u32> = s.frames().iter().map(|f| f.duration_ms).collect();
    let plan = VideoPlan::new(&durations);
    if plan.total_frames() > MAX_VIDEO_FRAMES {
        return Err(Error::Video(format!(
            "Das Video hätte {} Bilder, erlaubt sind {MAX_VIDEO_FRAMES}. Kürzere oder einheitlichere Frame-Dauern helfen.",
            plan.total_frames()
        )));
    }
    let (w, h) = export::check_export_size(s, scale)?;
    // Transparenz steht schon im unvergrößerten Bild. Die vergrößerten Bilder
    // entstehen erst beim Schreiben, eins nach dem anderen: Alle auf einmal
    // können mehrere Gigabyte belegen.
    let mut alpha = false;
    for f in 0..durations.len() {
        alpha |= s.composite(f)?.iter().any(|c| c.a < 255);
    }
    // ffmpeg schreibt in eine Datei daneben, die erst am Ende den richtigen
    // Namen bekommt. So bleibt ein früherer Export heil, wenn dieser scheitert.
    let name = out.file_stem().unwrap_or_default().to_string_lossy();
    let part = out.with_file_name(format!("{name}.teil-{}.webm", std::process::id()));
    let result = encode(s, scale, &plan, (w, h), alpha, &part, ffmpeg)
        .and_then(|()| std::fs::rename(&part, out).map_err(Error::from));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Startet ffmpeg und schickt ihm die Frames, jeden so oft, wie er dauert.
fn encode(
    s: &Sprite,
    scale: u32,
    plan: &VideoPlan,
    (w, h): (u32, u32),
    alpha: bool,
    out: &Path,
    ffmpeg: &Path,
) -> Result<()> {
    let failed = |what: String| Error::Video(format!("ffmpeg ({}): {what}", ffmpeg.display()));
    let mut command = Command::new(ffmpeg);
    command
        .args(ffmpeg_args(w, h, plan.tick_ms, alpha))
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // Kein aufblitzendes Konsolenfenster, wenn der Editor ffmpeg startet.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| failed(format!("lässt sich nicht starten: {e}")))?;
    // Die Fehlerausgabe wird nebenher gelesen. Sonst könnten beide Seiten
    // aufeinander warten: ffmpeg auf Platz für seine Meldungen, wir auf ffmpeg.
    let stderr = child.stderr.take();
    let messages = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = stderr {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).trim().to_string();
        }
        text
    });
    // Bricht ffmpeg vorzeitig ab, scheitert das Schreiben; die Ursache steht
    // dann in seiner Fehlerausgabe.
    let mut sent: Result<()> = Ok(());
    if let Some(mut pipe) = child.stdin.take() {
        'frames: for (f, &repeat) in plan.repeats.iter().enumerate() {
            let image = match export::frame_image(s, f, scale) {
                Ok(image) => image.into_raw(),
                Err(e) => {
                    sent = Err(e);
                    break;
                }
            };
            for _ in 0..repeat {
                if let Err(e) = pipe.write_all(&image) {
                    sent = Err(failed(e.to_string()));
                    break 'frames;
                }
            }
        }
    }
    let status = child.wait().map_err(|e| failed(e.to_string()))?;
    let message = messages.join().unwrap_or_default();
    if !status.success() {
        return Err(failed(if message.is_empty() {
            format!("beendet mit {status}")
        } else {
            message
        }));
    }
    sent
}
