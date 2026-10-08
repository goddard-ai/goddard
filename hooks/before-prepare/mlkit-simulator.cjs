/**
 * GoogleMLKit ships no arm64-simulator slices: MLKitVision's podspec sets
 * EXCLUDED_ARCHS[sdk=iphonesimulator*]=arm64 on both the pod targets and the
 * app target, so on Apple-silicon simulators the app cannot be installed while
 * the pod is linked.
 *
 * @nativescript/mlkit-barcode-scanning's Podfile omits GoogleMLKit/BarcodeScanning
 * when NSSkipMLKitFromIOSSimulator is present in the environment of the `pod
 * install` process, and mlkit-core's Swift + JS are all gated behind
 * `#if canImport(...)` / `typeof MLK* !== 'undefined'`, so skipping the pod
 * cleanly no-ops barcode scanning. This in-process before-prepare hook sets the
 * variable for simulator-targeted invocations; `pod install` inherits the env.
 *
 * Target detection:
 *   --for-device                 -> device, MLKit kept
 *   --device <simulator udid/name> -> simulator, pods skipped
 *   --device <physical/unknown>  -> device, MLKit kept (unresolvable values such
 *                                  as list indices are treated as physical)
 *   --emulator / no selector     -> simulator, pods skipped
 *
 * Escape hatches:
 *   NSSkipMLKitFromIOSSimulator=1 forces the skip on any command.
 *   NS_INCLUDE_MLKIT=1           forces MLKit in for commands that would
 *                                otherwise prepare a simulator build (e.g. a
 *                                bare `ns prepare ios` feeding a device
 *                                archive through xcodebuild).
 *
 * The env var is evaluated inside the Podfile, not by the CLI's change
 * detection, so flipping targets alone would leave stale Pods. When the
 * installed Podfile.lock disagrees with the target, this hook touches the
 * plugin's Podfile so the prepare marks native code changed and pod install
 * re-runs.
 *
 * Bare `ns run ios` with a physical device connected is prepared as a
 * simulator build (no MLKit); pass `--device <id>` for physical devices.
 */

const { execFileSync } = require('node:child_process');
const { existsSync, readFileSync, utimesSync } = require('node:fs');
const { join } = require('node:path');

const SKIP_ENV = 'NSSkipMLKitFromIOSSimulator';
const MLKIT_PLUGIN_PODFILE = join(
	'node_modules',
	'@nativescript',
	'mlkit-barcode-scanning',
	'platforms',
	'ios',
	'Podfile'
);

function isSimulatorIdentifier(value, logger) {
	try {
		const output = execFileSync('xcrun', ['simctl', 'list', 'devices', '--json'], {
			encoding: 'utf8',
		});

		const runtimes = JSON.parse(output).devices ?? {};
		for (const devices of Object.values(runtimes)) {
			for (const device of devices) {
				if (device.udid === value || device.name === value) {
					return true;
				}
			}
		}
	} catch (err) {
		logger?.trace(
			`mlkit-simulator: simctl lookup failed (${err.message}); treating --device as physical`
		);
	}

	return false;
}

module.exports = function ($options, $logger, hookArgs) {
	const platform = hookArgs?.prepareData?.platform ?? hookArgs?.projectData?.platform;
	if (String(platform).toLowerCase() !== 'ios') {
		return;
	}

	let skip;
	if (process.env[SKIP_ENV] || process.env.NS_INCLUDE_MLKIT) {
		skip = Boolean(process.env[SKIP_ENV]);
	} else if ($options.forDevice) {
		skip = false;
	} else if ($options.emulator) {
		skip = true;
	} else if ($options.device) {
		skip =
			typeof $options.device === 'string' &&
			isSimulatorIdentifier($options.device, $logger);
	} else {
		skip = true;
	}

	if (skip) {
		process.env[SKIP_ENV] = '1';
		$logger.info(
			'mlkit-simulator: iOS simulator build — skipping GoogleMLKit pods (barcode scanning unavailable; pass --for-device/--device or NS_INCLUDE_MLKIT=1 to keep them)'
		);
	}

	// Reconcile a stale Pods install when the target flips between simulator
	// and device: the env var changes Podfile evaluation, not its text, so the
	// CLI's mtime-based change detection would never notice.
	const projectDir = hookArgs?.projectData?.projectDir;
	if (!projectDir) {
		return;
	}

	const lockPath = join(projectDir, 'platforms', 'ios', 'Podfile.lock');
	if (!existsSync(lockPath)) {
		return;
	}

	const hasMLKit = /MLKit/.test(readFileSync(lockPath, 'utf8'));
	if (hasMLKit === skip) {
		try {
			const pluginPodfile = join(projectDir, MLKIT_PLUGIN_PODFILE);
			const now = new Date();
			utimesSync(pluginPodfile, now, now);
			$logger.info(
				'mlkit-simulator: Pods state is stale for this target — forcing pod install'
			);
		} catch (err) {
			$logger.warn(
				`mlkit-simulator: Pods look stale for this target but re-install could not be triggered (${err.message}); run \`ns clean\``
			);
		}
	}
};
