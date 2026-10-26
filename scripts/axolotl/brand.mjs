import { spawn } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const brands = JSON.parse(fs.readFileSync(path.join(root, 'brands/catalog.json'), 'utf8'))
const [id = 'ysumc', action = 'preview', version] = process.argv.slice(2)
if (!Object.hasOwn(brands, id)) throw new Error(`Unknown brand: ${id}`)
const brand = brands[id]

if (action === 'preview') {
    const frontend = path.join(root, 'apps/app-frontend')
    const child = spawn(process.execPath, [path.join(frontend, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1'], {
        cwd: frontend,
        env: { ...process.env, VITE_LAUNCHER_BRAND: id },
        stdio: 'inherit',
        windowsHide: true,
    })
    child.on('error', (error) => { console.error(error); process.exitCode = 1 })
    child.on('exit', (code) => { process.exitCode = code ?? 1 })
    console.log(`Preview: http://127.0.0.1:5201/${id === 'mutmc' ? 'preview.html' : ''}`)
} else if (action === 'config') {
    if (version && !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
        throw new Error('A semantic version is required')
    }
    const base = JSON.parse(fs.readFileSync(path.join(root, 'apps/app/tauri.conf.json'), 'utf8'))
    console.log(JSON.stringify({
        productName: brand.productName,
        mainBinaryName: brand.productName,
        identifier: brand.identifier,
        ...(version ? { version } : {}),
        ...(id === 'mutmc' ? { bundle: {
            icon: ['icons/mutmc/128x128.png', 'icons/mutmc/128x128@2x.png', 'icons/mutmc/icon.icns', 'icons/mutmc/icon.ico'],
            shortDescription: 'MUTMCL Minecraft Launcher',
        } } : {}),
        app: { windows: base.app.windows.map((window) => ({ ...window, title: brand.productName })) },
        plugins: {
            'deep-link': { desktop: { schemes: [brand.scheme] } },
            updater: { endpoints: [`${brand.apiBase}/update/{{target}}/{{arch}}/{{current_version}}`] },
        },
    }, null, 2))
} else {
    throw new Error('Use: node scripts/axolotl/brand.mjs <ysumc|mutmc> <preview|config> [version]')
}
