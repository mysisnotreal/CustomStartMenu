# Fonts for `> font`

- `catalog.tsv` lists every font the picker shows: category, clean name, id, download link, size, sample script, flags (`c` = color font). Built into both apps.
- `previews/<id>.png` are the card pictures, drawn with DirectWrite. Grayscale pictures are coverage that the app colors with the theme; the color font (Twemoji) is a normal picture.

The fonts themselves are downloaded from their own repositories only when someone clicks Download:

- [sahibjotsaggu/San-Francisco-Pro-Fonts](https://github.com/sahibjotsaggu/San-Francisco-Pro-Fonts) (SF Pro)
- [kiwi0fruit/open-fonts](https://github.com/kiwi0fruit/open-fonts) (everything else)

To rebuild after the source repositories change:

```bat
python tools\fonts\make_catalog.py
python tools\fonts\render_previews.py build\font-cache
```
