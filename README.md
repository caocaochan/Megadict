# Megadict

One Chinese term dictionary for Yomitan, assembled from caocaochan’s published dictionaries. Each definition carries its source name, and the original headwords, readings, examples, cross-references, and images remain available.

## Install and update

Download [Megadict.zip](https://github.com/caocaochan/Megadict/releases/latest/download/Megadict.zip) and import it under **Yomitan → Settings → Dictionaries → Import** with a Chinese profile selected. For subsequent releases, use **Check for updates** in Yomitan’s dictionary manager. The ZIP embeds the [stable update index](https://github.com/caocaochan/Megadict/releases/latest/download/index.json).

Megadict is a large import because it includes the full HYDCD with images and the full Chinese Wikipedia dictionary. Lookups use Yomitan’s normal result grouping and display the source name above each definition.

## Sources

- [现代汉语大词典](https://github.com/caocaochan/xhdcd-yomitan) and [汉语大词典 七訂](https://github.com/caocaochan/hydcd-yomitan)
- [新华成语大词典 and 汉语成语源流大辞典](https://github.com/caocaochan/chengyu-yomitan)
- [Mandarin Collocations](https://github.com/caocaochan/mandarin-collocations), [Bilibili Slang](https://github.com/caocaochan/bilibili-slang-dictionary), and [樂詞網](https://github.com/caocaochan/naer-yomitan)
- [CBDB 人物](https://github.com/caocaochan/cbdb-yomitan) and [NovelNames](https://github.com/caocaochan/NovelNames)
- [Genshin Impact, Chinese and English editions](https://github.com/caocaochan/genshin-yomitan), [萌娘百科](https://github.com/caocaochan/moegirl-yomitan), and [Chinese Wikipedia](https://github.com/caocaochan/chinese-wikipedia-dictionary)

The [source manifest](https://github.com/caocaochan/Megadict/releases/latest/download/sources.json) records the exact input release, SHA-256, and included row count for each dictionary. Data attribution and accompanying notices are included in the ZIP. Each source retains its own data license.

Install the [Chinese frequency packs](https://github.com/caocaochan/yomitan-frequency-dicts/releases/latest) individually to keep their corpus names and rank settings in Yomitan.

## Build

Assumes PowerShell 7 and stable Rust. On Windows, use the MSVC toolchain.

```powershell
./build.ps1 -Refresh
```

The build checks GitHub release SHA-256 hashes and reuses matching archives from the cache or sibling checkouts. The output is `dist/Megadict.zip`, with identical release metadata extracted alongside it and the original frequency packs under `dist/frequencies`. Run `./build.ps1` again to rebuild the cached snapshot.

GitHub Actions checks the upstream releases hourly. A changed archive triggers a rebuild and full validation using Yomitan’s release schemas. The workflow publishes the ZIP and its update index together on a GitHub release, so the index always points to a specific uploaded ZIP.
