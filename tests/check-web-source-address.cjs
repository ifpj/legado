const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const code = fs.readFileSync(path.resolve(__dirname, '../web/app.js'), 'utf8');

function page(origin, storedBase = '') {
    const elements = new Map(), saved = new Map();
    if (storedBase) saved.set('relay.base', storedBase);
    const node = () => ({value: '', listeners: {}, style: {}, dataset: {},
        classList: {add() {}, remove() {}, toggle() {}},
        append() {}, replaceChildren() {}, setAttribute() {},
        addEventListener(name, action) {this.listeners[name] = action;}});
    const element = id => {
        if (!elements.has(id)) elements.set(id, node());
        return elements.get(id);
    };
    element('operation').value = 'search';
    const context = vm.createContext({URL, TextEncoder, location: new URL(origin),
        document: {getElementById: element, querySelectorAll: () => [],
            createElement: node, createElementNS: node},
        window: {addEventListener() {}},
        sessionStorage: {getItem: key => saved.get(key), setItem: (key, value) => saved.set(key, value)},
        fetch: () => new Promise(() => {}), setInterval() {}, setTimeout() {}, clearTimeout() {}});
    vm.runInContext(code, context);
    function status(publicUrl, tokenEnabled = false, configuredPublicUrl = null) {
        context.snapshot = {successRate: 100, requests: [], connections: [], service: {
            publicUrl, configuredPublicUrl, tokenEnabled, version: 'test', platform: 'linux', arch: 'x86_64'}};
        vm.runInContext(`renderTrend=renderOperations=renderRequests=renderAllRequests=renderConnections=()=>{};
            runtimeRows=()=>[];state.data=snapshot;renderStatus();`, context);
    }
    function check(expected) {
        assert.equal(element('public-base').value, expected);
        const source = new URL(element('source-url').textContent);
        assert.equal(source.origin, expected);
        assert.equal(source.pathname, '/source.json');
        assert.equal(source.searchParams.get('base'), expected);
        assert.equal(new URL(element('qr-image').src, origin).searchParams.get('base'), expected);
        assert.equal(new URL(element('import-source').href).searchParams.get('src'), source.href);
    }
    return {element, check, status, saved};
}

for (const origin of ['http://relay.lan:122', 'http://1.1.1.1:8088', 'https://books.example', 'http://[::1]:122']) {
    const ui = page(origin);
    ui.check(origin); // Import is usable before the status request or token entry completes.
    ui.status(origin);
    ui.check(origin);
    ui.status('http://container:19670');
    ui.check(origin); // Browser HTTPS remains correct even without proxy headers.
    ui.status('https://fixed.example', false, 'https://fixed.example');
    ui.check('https://fixed.example');
    ui.element('public-base').value = 'http://manual.lan:8088/';
    ui.element('apply-address').listeners.click();
    ui.status(origin, true);
    ui.check('http://manual.lan:8088');
    assert.equal(ui.saved.get('relay.base'), 'http://manual.lan:8088');
    assert.equal(ui.element('import-source').hidden, true);
}
const manual = page('https://books.example', 'http://manual.lan:122');
manual.status('https://fixed.example', false, 'https://fixed.example');
manual.check('http://manual.lan:122');
console.log('Web source address checks passed: host/port/HTTPS/IPv6, initial import, configured/manual override, token.');
