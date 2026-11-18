//! Dialog „Farbvarianten…“: Skins und Teamfarben einer Figur. Eine Variante
//! tauscht Farben und gilt für die Figur mit allen Animationen, die Zeichnung
//! selbst gibt es nur einmal.

use crate::app::{c32, section, App};
use crate::dialogs::{ExportForm, Outcome};
use crate::doc::{AssetId, AssetKind};
use crate::player::{self, Player};
use eframe::egui::{self, Color32, RichText};
use pixel_core::lang::tr;
use pixel_core::variants::{is_colorful, ColorVariant};
use pixel_core::{trf, Rgba};
use uuid::Uuid;

const PREVIEW: f32 = 200.0;

pub struct VariantForm {
    character: Uuid,
    /// Was die Vorschau abspielt: die Figur oder eine ihrer Animationen.
    shown: AssetId,
    draft: Vec<ColorVariant>,
    selected: usize,
    new_name: String,
    hue: f32,
    /// Seit dem letzten Übernehmen geändert.
    dirty: bool,
    original: Option<Player>,
    turned: Option<Player>,
}

impl VariantForm {
    pub fn new(app: &App, id: AssetId) -> Option<Self> {
        let doc = app.doc.as_ref()?;
        let character = doc.character_of(id)?;
        let shown = if id.kind == AssetKind::Animation {
            id
        } else {
            doc.assets()
                .into_iter()
                .find(|a| a.character == Some(character))
                .map_or(AssetId::new(AssetKind::Character, character), |a| a.id)
        };
        let draft = doc.variants(character).to_vec();
        Some(Self {
            character,
            shown,
            new_name: if draft.is_empty() {
                tr("Rot", "Red").to_string()
            } else {
                String::new()
            },
            draft,
            selected: 0,
            hue: 120.0,
            dirty: false,
            original: None,
            turned: None,
        })
    }
}

/// Die Farben der Figur und ihrer Animationen, die tatsächlich vorkommen.
fn colors(app: &App, character: Uuid) -> Vec<Rgba> {
    let Some(doc) = app.doc.as_ref() else {
        return Vec::new();
    };
    let mut out: Vec<Rgba> = Vec::new();
    for a in doc.assets() {
        if a.id.id == character || a.character == Some(character) {
            if let Some(s) = doc.sprite(a.id) {
                for c in s.used_colors() {
                    let c = Rgba::rgb(c.r, c.g, c.b);
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

pub fn show(app: &mut App, ui: &mut egui::Ui, f: &mut VariantForm) -> Outcome {
    let Some(doc) = app.doc.as_ref() else {
        return Outcome::Close;
    };
    let char_id = AssetId::new(AssetKind::Character, f.character);
    let Some(name) = doc.asset_name(char_id).map(str::to_string) else {
        return Outcome::Close;
    };
    let shows: Vec<(AssetId, String)> = doc
        .assets()
        .into_iter()
        .filter(|a| a.id.id == f.character || a.character == Some(f.character))
        .map(|a| (a.id, a.name))
        .collect();
    let palette = colors(app, f.character);
    ui.heading(trf!(
        "Farbvarianten von „{name}“",
        "Color variants of \"{name}\""
    ));
    ui.label(tr(
        "Eine Variante tauscht Farben, etwa für Teams oder Skins. Sie gilt für die Figur und alle Animationen; was du an der Figur zeichnest, ist sofort in jeder Variante. Beim Export wählst du die Variante aus.",
        "A variant swaps colors, for teams or skins. It applies to the character and all animations; whatever you draw on the character is in every variant right away. Pick the variant when exporting.",
    ));
    ui.add_space(4.0);
    // Varianten zur Auswahl, dazu eine neue anlegen.
    ui.horizontal_wrapped(|ui| {
        for (i, v) in f.draft.iter().enumerate() {
            if ui.selectable_label(i == f.selected, &v.name).clicked() {
                f.selected = i;
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut f.new_name)
                .desired_width(110.0)
                .hint_text(tr("Name", "Name")),
        );
        let name = f.new_name.trim().to_string();
        let taken = f.draft.iter().any(|v| v.name.eq_ignore_ascii_case(&name));
        if ui
            .add_enabled(
                !name.is_empty() && !taken,
                egui::Button::new(tr("+ Variante", "+ Variant")),
            )
            .clicked()
        {
            let mut v = ColorVariant::new(name);
            // Ein Startvorschlag: alle bunten Farben im Farbkreis gedreht.
            let colorful: Vec<Rgba> = palette
                .iter()
                .copied()
                .filter(|c| is_colorful(*c))
                .collect();
            v.shift_hue(&colorful, 120.0 * (1 + f.draft.len()) as f32);
            f.draft.push(v);
            f.selected = f.draft.len() - 1;
            f.new_name.clear();
            f.dirty = true;
        }
    });
    if f.draft.is_empty() {
        ui.weak(tr(
            "Noch keine Variante. Gib oben einen Namen ein.",
            "No variant yet. Enter a name above.",
        ));
    }
    f.selected = f.selected.min(f.draft.len().saturating_sub(1));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(330.0);
            if let Some(v) = f.draft.get_mut(f.selected) {
                section(ui, tr("Farben", "Colors"));
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .min_scrolled_height(260.0).show(ui, |ui| {
                    egui::Grid::new("variant-colors").num_columns(4).show(ui, |ui| {
                        for &c in &palette {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 3.0, c32(c));
                            ui.label(">");
                            let to = v.map(c);
                            let mut rgb = [to.r, to.g, to.b];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                v.set(c, Rgba::rgb(rgb[0], rgb[1], rgb[2]));
                                f.dirty = true;
                            }
                            if to != c {
                                if ui.small_button(tr("zurück", "reset")).clicked() {
                                    v.set(c, c);
                                    f.dirty = true;
                                }
                            } else {
                                ui.weak(c.to_string());
                            }
                            ui.end_row();
                        }
                    });
                });
                section(ui, tr("Farbton drehen", "Turn the hue"));
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut f.hue, -180.0..=180.0).suffix("°"));
                    if ui
                        .button(tr("Auf alle bunten Farben", "To all colorful colors"))
                        .on_hover_text(tr(
                            "Dreht den Farbton aller deutlich bunten Originalfarben; Grau, Umriss und Glanzlichter bleiben, Helligkeit und Sättigung auch.",
                            "Turns the hue of all clearly colorful original colors; grays, outline and highlights stay, and so do lightness and saturation.",
                        ))
                        .clicked()
                    {
                        let hue = f.hue;
                        let picked: Vec<Rgba> = palette
                            .iter()
                            .copied()
                            .filter(|c| is_colorful(*c))
                            .collect();
                        v.shift_hue(&picked, hue);
                        f.dirty = true;
                    }
                });
                ui.add_space(4.0);
                if ui.button(trf!("Variante „{}“ löschen", "Delete variant \"{}\"", v.name)).clicked() {
                    f.draft.remove(f.selected);
                    f.selected = f.selected.saturating_sub(1);
                    f.dirty = true;
                }
            }
        });
        ui.add_space(8.0);
        ui.vertical(|ui| {
            let label = shows
                .iter()
                .find(|(id, _)| *id == f.shown)
                .map_or(String::new(), |(_, n)| n.clone());
            egui::ComboBox::from_id_salt("variant-shown")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, n) in &shows {
                        ui.selectable_value(&mut f.shown, *id, n);
                    }
                });
            previews(app, ui, f);
        });
    });
    ui.add_space(8.0);
    let mut outcome = Outcome::Keep;
    ui.horizontal(|ui| {
        let save = |app: &mut App, f: &mut VariantForm| -> bool {
            let done = app
                .doc
                .as_mut()
                .map(|d| d.set_variants(f.character, f.draft.clone()));
            match done {
                Some(Ok(())) => {
                    f.dirty = false;
                    true
                }
                Some(Err(err)) => {
                    app.dialog_error = Some(err);
                    false
                }
                None => false,
            }
        };
        if ui
            .add_enabled(
                f.dirty,
                egui::Button::new(RichText::new(tr("Übernehmen", "Apply")).strong()),
            )
            .clicked()
            && save(app, f)
        {
            app.info(tr("Farbvarianten gespeichert", "Color variants saved"));
        }
        if ui
            .add_enabled(
                !f.draft.is_empty(),
                egui::Button::new(tr("Exportieren…", "Export…")),
            )
            .on_hover_text(tr(
                "Speichert und öffnet den Export mit dieser Variante.",
                "Saves and opens the export with this variant.",
            ))
            .clicked()
            && (!f.dirty || save(app, f))
        {
            let mut form = ExportForm {
                variant: f.draft.get(f.selected).map(|v| v.name.clone()),
                ..ExportForm::default()
            };
            form.kind = crate::dialogs::ExportKind::Sheet;
            app.select(f.shown);
            app.open_dialog(crate::dialogs::Dialog::Export(form));
            outcome = Outcome::Close;
        }
        let close = if f.dirty {
            tr("Verwerfen und schließen", "Discard and close")
        } else {
            tr("Schließen", "Close")
        };
        if ui.button(close).clicked() {
            outcome = Outcome::Close;
        }
    });
    outcome
}

fn previews(app: &App, ui: &mut egui::Ui, f: &mut VariantForm) {
    let Some(doc) = app.doc.as_ref() else { return };
    let Some(sprite) = doc.sprite(f.shown) else {
        return;
    };
    let revision = doc.revision();
    let variant = f.draft.get(f.selected).cloned();
    Player::ensure(
        &mut f.original,
        ui,
        "variant-original",
        player::key(&(revision, f.shown)),
        || Some(sprite.clone()),
    );
    Player::ensure(
        &mut f.turned,
        ui,
        "variant-turned",
        player::key(&(revision, f.shown, &variant)),
        || {
            Some(
                variant
                    .as_ref()
                    .map_or_else(|| sprite.clone(), |v| v.apply(sprite)),
            )
        },
    );
    ui.horizontal(|ui| {
        for (p, label) in [
            (&f.original, tr("Original", "Original").to_string()),
            (
                &f.turned,
                variant
                    .as_ref()
                    .map_or_else(String::new, |v| v.name.clone()),
            ),
        ] {
            ui.vertical(|ui| {
                let (rect, _) =
                    player::frame(ui, egui::vec2(PREVIEW, PREVIEW), egui::Sense::hover());
                if let Some(p) = p {
                    p.paint(ui, &ui.painter_at(rect), rect);
                }
                ui.label(RichText::new(label).color(Color32::from_gray(190)));
            });
        }
    });
}
