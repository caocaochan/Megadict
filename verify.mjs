// Assumes Node 24 and the lib files from the pinned Yomitan release.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const [yomitanDirectory, distDirectory] = process.argv.slice(2);
assert(yomitanDirectory && distDirectory, 'Usage: node verify.mjs <Yomitan-directory> <dist-directory>');

const loadYomitan = (file) => import(pathToFileURL(resolve(yomitanDirectory, 'lib', file)));
const {dictionaryIndex, dictionaryTermBankV3} = await loadYomitan('validate-schemas.js');
const {TextWriter, Uint8ArrayReader, ZipReader} = await loadYomitan('zip.js');
const readDist = (file, options) => readFile(resolve(distDirectory, file), options);
const archive = new ZipReader(new Uint8ArrayReader(await readDist('Megadict.zip')));

try {
    const list = await archive.getEntries();
    const entries = new Map(list.map((entry) => [entry.filename, entry]));
    assert.equal(entries.size, list.length, 'Duplicate ZIP members');
    const readText = (entry) => entry.getData(new TextWriter(), {checkSignature: true, useWebWorkers: false});

    const indexText = await readText(entries.get('index.json'));
    const index = JSON.parse(indexText);
    assert(dictionaryIndex(index), JSON.stringify(dictionaryIndex.errors));
    assert.equal(index.title, 'Megadict');
    assert.equal(index.isUpdatable, true);
    assert.equal(index.indexUrl, 'https://github.com/caocaochan/Megadict/releases/latest/download/index.json');
    assert.equal(index.downloadUrl, `https://github.com/caocaochan/Megadict/releases/download/v${index.revision}/Megadict.zip`);
    assert.equal(indexText, await readDist('index.json', 'utf8'), 'Remote index must match the embedded index');

    const manifestText = await readText(entries.get('sources.json'));
    assert.equal(manifestText, await readDist('sources.json', 'utf8'));
    const sources = JSON.parse(manifestText);
    const counts = new Map(sources.map(({id, title}) => [id, {title, rows: 0}]));
    assert.equal(counts.size, sources.length, 'Duplicate sources');
    assert(!counts.has('cedict'));
    const imagePaths = new Set();
    let currentSource;
    let labels = 0;

    function visit(content) {
        if (Array.isArray(content)) {
            for (const child of content) visit(child);
        } else if (content && typeof content === 'object') {
            const label = content.data?.['megadict-label'];
            if (label) {
                const source = counts.get(label);
                assert(source, `Unexpected source ${label}`);
                assert.equal(content.content, source.title);
                currentSource = label;
                labels++;
            }
            if (content.tag === 'img') {
                assert(entries.has(content.path), `Missing asset ${content.path}`);
                imagePaths.add(content.path);
            }
            visit(content.content);
        }
    }

    const bankCount = list.filter(({filename}) => /^term_bank_\d+\.json$/.test(filename)).length;
    assert(bankCount > 0);
    for (let number = 1; number <= bankCount; number++) {
        const name = `term_bank_${number}.json`;
        const bank = JSON.parse(await readText(entries.get(name)));
        assert(dictionaryTermBankV3(bank), `${name}: ${JSON.stringify(dictionaryTermBankV3.errors)}`);
        for (const row of bank) {
            labels = 0;
            for (const gloss of row[5]) visit(gloss.content);
            assert.equal(labels, 1, `${name}: expected one source label for ${row[0]}`);
            counts.get(currentSource).rows++;
        }
    }
    let rows = 0;
    for (const source of sources) {
        assert.equal(counts.get(source.id).rows, source.rows, source.title);
        rows += source.rows;
    }
    for (const path of imagePaths) {
        assert(entries.get(path).uncompressedSize > 0, `Empty asset ${path}`);
    }
    console.log(`Yomitan validated ${rows} rows in ${bankCount} banks from ${sources.length} sources, with ${imagePaths.size} referenced images.`);
} finally {
    await archive.close();
}
