const fs = require('node:fs')
const path = require('node:path')

const checkout = path.join(__dirname, 'octane-xplat')

module.exports = {
	hooks: {
		readPackage(pkg) {
			if (pkg.name !== 'octane-xplat-app') {
				return pkg
			}

			for (const field of ['dependencies', 'devDependencies', 'optionalDependencies']) {
				for (const name of Object.keys(pkg[field] || {})) {
					if (!name.startsWith('@octane-xplat/')) {
						continue
					}

					const directory = path.join(checkout, 'packages', name.slice('@octane-xplat/'.length))
					const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'))
					if (manifest.name !== name) {
						throw new Error(`Expected ${name} in ${directory}`)
					}

					pkg[field][name] = `link:${directory}`
				}
			}

			return pkg
		},
	},
}
