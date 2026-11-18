# Was ist neu

## 1.4.0

- Kachel-Modus (Taste T): die Leinwand zeigt die Nachbarkacheln, und Striche über den Rand kommen auf der
  anderen Seite wieder herein
- Autotile: aus einer Vorlage mit fünf gemalten Kacheln entsteht ein ganzer Satz mit 47 oder 16 Kacheln,
  mit Beispielkarte in der Vorschau und animierten Kacheln aus mehreren Frames
- Export als TileSet für Godot 4 (mit Terrain) und für Tiled (mit Wang-Set)
- Neuer Dialog **Autotile…**
- Für Claude: neues Werkzeug `autotile`, und `export` kennt die Formate `godot` und `tiled`

## 1.3.0

- Effekte auf eigenen Ebenen, die über die Frames laufen und jeder Änderung folgen: Leuchten, Blitz, Explosion,
  Spiegelung im Wasser, Funken, Rauch und Magie
- Farbzyklen (Palettenanimation) mit den Vorlagen Wasser, Feuer und Lava
- Neuer Dialog **Effekte…** mit abspielender Vorschau; aus einem Standbild wird auf Wunsch eine Schleife
- Für Claude: neues Werkzeug `effects`

## 1.2.0

- Stil-Bibliothek: Comic, Manga, Burgen, 8 Bit, 16 Bit und HQ Pixel-Art mit Palette, Größen, Umriss,
  Schattierung und Regeln; eigene Stile anlegen und speichern
- Für Claude: neues Werkzeug `styles`, und `create_image`, `create_character` und `set_style` nehmen `style`
- Einstellungen mit Tabs: Allgemein, Claude verbinden, Stile und Tipps
- Normal Maps für Licht in Godot und Unity, mit Vorschau und Lampe; für Claude über `export`
- Projektauswahl links über der Liste, neuer Über-Dialog mit Version und Ko-fi-Link

## 1.1.0

- Licht und Schatten auf eigenen Ebenen, die jeder Änderung folgen, für Bilder, Animationen und Figuren
- Die Lichtquelle kann über die Frames wandern, etwa eine vorbeiziehende Fackel
- Schlagschatten am Boden, der vom Licht weg fällt
- Neuer Dialog **Licht…** mit Vorschau je Frame und Lichtpfad
- Für Claude: neues Werkzeug `set_light`

## 1.0.0 Beta

- Fotos und Zeichnungen beim Import in Pixel-Art umwandeln: Zielgröße, Farben oder Projektpalette,
  Dithering, Hintergrund entfernen, Umriss, kräftigere Farben, Vorschau vorher und nachher
- Vorlage-Ebene zum Nachzeichnen, die nicht exportiert wird
- Beides auch für Claude über `import_image`
- Feature-Übersicht mit Bildern in der README

## 0.10.0

- Formen (Herz, Raute, Stern, Dreieck, Kreuz, Sechseck, Pfeil, Kreis) und eigene Formen
- Sprühdose, Aufhellen/Abdunkeln, Auswahl mit Kopieren, Ausschneiden und Einfügen
- Spiegeln beim Zeichnen, Pixel-perfekter Stift, Farbe ersetzen, Leinwandgröße
- Paletten mit 32 und 64 Farben, neue Werkzeugleiste mit Symbolen

## 0.9.0

- Automatische Konturen, Projekte löschen, aufgeräumte Seitenleisten, Programmsymbol

## 0.8.0

- Einstellungen mit Speicherordner, Deutsch und Englisch, Werkzeug-Symbole

## 0.7.0

- Prüf-Hinweise, Paletten aus Bildern, Farbreduktion, Projekt-Stil

## 0.6.0 und früher

- Zeichentechniken für Claude, Import von PNG, JPG, GIF und Spritesheets
- Eine exe für Editor und MCP-Server, ffmpeg aus der App installieren
- GIF-, WebM- und PNG-8-Export, Editor mit Live-Abgleich, MCP-Server, Kern mit Projektformat
