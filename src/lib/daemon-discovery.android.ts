import { Utils } from '@nativescript/core';
import { TextDecoder } from '@nativescript/core/text';

import type { DiscoveredDaemon } from './daemon-discovery.web';

export type { DiscoveredDaemon };

// waku-core advertises `_waku._tcp.local.` while the daemon listens on a
// non-loopback address; the TXT record carries `id` (the share-endpoint id)
// and `name` alongside `v`/`pv` version keys.
const SERVICE_TYPE = '_waku._tcp.';

const decoder = new TextDecoder();

function txtString(
	attributes: java.util.Map<string, androidNative.Array<number>> | null,
	key: string,
): string | undefined {
	const bytes = attributes?.get(key);
	if (!bytes || bytes.length === 0) {return undefined;}
	const utf8 = new Uint8Array(bytes.length);
	for (let index = 0; index < bytes.length; index++) {
		utf8[index] = bytes[index] & 0xff;
	}

	return decoder.decode(utf8);
}

/** Prefer a dotted quad; an IPv6 literal gets brackets so it stays a valid
 * `ws://` host. `getHostAddresses` only exists on API 34+, so it goes
 * through a cast — the project types target API 31. */
function hostFromService(info: android.net.nsd.NsdServiceInfo): string | null {
	const candidates: java.net.InetAddress[] = [];
	if (android.os.Build.VERSION.SDK_INT >= 34) {
		const list = (
			info as unknown as {
				getHostAddresses(): java.util.List<java.net.InetAddress>;
			}
		).getHostAddresses();

		if (list) {
			for (let index = 0; index < list.size(); index++) {
				candidates.push(list.get(index));
			}
		}
	}

	const legacy = info.getHost();
	if (legacy) {candidates.push(legacy);}
	const addresses = candidates
		.map((candidate) => candidate?.getHostAddress())
		.filter((address): address is string => !!address);

	const host = addresses.find((address) => !address.includes(':')) ?? addresses[0];
	if (!host) {return null;}
	return host.includes(':') ? `[${host}]` : host;
}

/** Browse `_waku._tcp` for daemons advertising on the LAN. `onChange`
 * receives the whole current set each time a service resolves or
 * disappears; the returned function stops discovery. */
export function subscribeDaemonDiscovery(
	onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
	const context = Utils.android.getApplicationContext();
	const nsd = context.getSystemService(
		android.content.Context.NSD_SERVICE,
	) as android.net.nsd.NsdManager;

	let stopped = false;
	let resolving = false;
	const daemons = new Map<string, DiscoveredDaemon>();
	const wanted = new Set<string>();
	const pending: android.net.nsd.NsdServiceInfo[] = [];

	const emit = () => {
		if (!stopped) {onChange([...daemons.values()]);}
	};

	const upsert = (info: android.net.nsd.NsdServiceInfo) => {
		const name = info.getServiceName();
		const host = hostFromService(info);
		const port = info.getPort();
		if (!host || !port) {return;}
		const attributes = info.getAttributes();
		daemons.set(name, {
			id: txtString(attributes, 'id') ?? name,
			name: txtString(attributes, 'name') || name,
			address: `ws://${host}:${port}`,
		});

		emit();
	};

	// NsdManager resolves one service at a time — concurrent resolveService
	// calls fail with FAILURE_ALREADY_ACTIVE, so requests queue here.
	const drain = () => {
		if (stopped || resolving) {return;}
		const next = pending.shift();
		if (!next) {return;}
		resolving = true;
		nsd.resolveService(next, resolveListener);
	};

	const resolveListener = new android.net.nsd.NsdManager.ResolveListener({
		onResolveFailed: (info) => {
			resolving = false;
			// Let the next onServiceFound retry it.
			wanted.delete(info.getServiceName());
			drain();
		},
		onServiceResolved: (info) => {
			resolving = false;
			if (wanted.has(info.getServiceName())) {upsert(info);}
			drain();
		},
		onResolutionStopped: () => {},
		onStopResolutionFailed: (_info, _errorCode) => {},
	});

	const discoveryListener = new android.net.nsd.NsdManager.DiscoveryListener({
		onServiceFound: (info) => {
			const name = info.getServiceName();
			if (stopped || wanted.has(name)) {return;}
			wanted.add(name);
			pending.push(info);
			drain();
		},
		onServiceLost: (info) => {
			const name = info.getServiceName();
			wanted.delete(name);
			if (daemons.delete(name)) {emit();}
		},
		onDiscoveryStarted: () => {},
		onDiscoveryStopped: () => {},
		onStartDiscoveryFailed: (_type, errorCode) => {
			console.log(`daemon discovery failed to start: ${errorCode}`);
		},
		onStopDiscoveryFailed: (_type, errorCode) => {
			console.log(`daemon discovery failed to stop: ${errorCode}`);
		},
	});

	nsd.discoverServices(
		SERVICE_TYPE,
		android.net.nsd.NsdManager.PROTOCOL_DNS_SD,
		discoveryListener,
	);

	return () => {
		stopped = true;
		pending.length = 0;
		wanted.clear();
		daemons.clear();
		nsd.stopServiceDiscovery(discoveryListener);
	};
}
