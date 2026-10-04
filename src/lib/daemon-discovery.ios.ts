import { TextDecoder } from '@nativescript/core/text';

import type { DiscoveredDaemon } from './daemon-discovery.web';

export type { DiscoveredDaemon };

// waku-core advertises `_waku._tcp.local.` while the daemon listens on a
// non-loopback address; the TXT record carries `id` (the share-endpoint id)
// and `name` alongside `v`/`pv` version keys.
const SERVICE_TYPE = '_waku._tcp.';
const SERVICE_DOMAIN = 'local.';
const RESOLVE_TIMEOUT_SECONDS = 10;
const AF_INET = 2;

const decoder = new TextDecoder();

/** First dotted quad in the resolved address list. `service.addresses` is an
 * array of raw `sockaddr` structs: byte 1 is the family, and in sockaddr_in
 * the IPv4 address sits at bytes 4–7. */
function ipv4FromAddresses(addresses: NSArray<NSData>): string | null {
	if (!addresses) {return null;}
	for (let index = 0; index < addresses.count; index++) {
		const bytes = new Uint8Array(
			interop.bufferFromData(addresses.objectAtIndex(index)),
		);

		if (bytes.length >= 8 && bytes[1] === AF_INET) {
			return `${bytes[4]}.${bytes[5]}.${bytes[6]}.${bytes[7]}`;
		}
	}

	return null;
}

function txtString(
	txt: NSDictionary<string, NSData> | null,
	key: string,
): string | undefined {
	const data = txt?.objectForKey(key);
	return data?.length
		? decoder.decode(new Uint8Array(interop.bufferFromData(data)))
		: undefined;
}

/** One delegate serves the browser and every in-flight service resolution —
 * both protocol surfaces land here, keyed by service instance name. */
@NativeClass
class DaemonBrowserDelegate
	extends NSObject
	implements NSNetServiceBrowserDelegate, NSNetServiceDelegate
{
	static ObjCProtocols = [NSNetServiceBrowserDelegate, NSNetServiceDelegate];

	static initWithCallback(
		onChange: (daemons: DiscoveredDaemon[]) => void,
	): DaemonBrowserDelegate {
		const delegate = DaemonBrowserDelegate.new() as DaemonBrowserDelegate;
		// `new()` takes the ObjC alloc/init path — the JS constructor (and its
		// field initializers) never runs, so every field is assigned here.
		// Plain-object state: Map/Set stored on a NativeClass delegate get
		// bridged into a proxy whose iteration breaks.
		delegate.onChange = onChange;
		delegate.stopped = false;
		delegate.live = Object.create(null) as Record<string, true>;
		delegate.resolved = Object.create(null) as Record<string, DiscoveredDaemon>;
		delegate.pending = Object.create(null) as Record<string, NSNetService>;
		return delegate;
	}

	onChange!: (daemons: DiscoveredDaemon[]) => void;
	stopped = false;
	private live!: Record<string, true>;
	private resolved!: Record<string, DiscoveredDaemon>;
	private pending!: Record<string, NSNetService>;

	private emit(): void {
		if (!this.stopped) {this.onChange(Object.values(this.resolved));}
	}

	netServiceBrowserDidFindServiceMoreComing(
		_browser: NSNetServiceBrowser,
		service: NSNetService,
	): void {
		const key = String(service.name);
		this.live[key] = true;
		this.pending[key] = service;
		service.delegate = this;
		service.resolveWithTimeout(RESOLVE_TIMEOUT_SECONDS);
	}

	netServiceBrowserDidNotSearch(
		_browser: NSNetServiceBrowser,
		errorDict: NSDictionary<string, number>,
	): void {
		console.log(`daemon discovery failed: ${errorDict}`);
	}

	netServiceBrowserDidRemoveServiceMoreComing(
		_browser: NSNetServiceBrowser,
		service: NSNetService,
	): void {
		const key = String(service.name);
		delete this.live[key];
		if (key in this.resolved) {
			delete this.resolved[key];
			this.emit();
		}
	}

	netServiceDidResolveAddress(service: NSNetService): void {
		const key = String(service.name);
		delete this.pending[key];
		if (this.stopped || !(key in this.live)) {return;}
		const host =
			ipv4FromAddresses(service.addresses) ??
			service.hostName?.replace(/\.$/, '');

		const port = service.port;
		if (!host || !port) {return;}
		const record = service.TXTRecordData();
		const txt = record
			? NSNetService.dictionaryFromTXTRecordData(record)
			: null;

		this.resolved[key] = {
			id: txtString(txt, 'id') ?? service.name,
			name: txtString(txt, 'name') || service.name,
			address: `ws://${host}:${port}`,
		};

		this.emit();
	}

	netServiceDidNotResolve(service: NSNetService): void {
		delete this.pending[String(service.name)];
	}

	stop(): void {
		this.stopped = true;
		for (const service of Object.values(this.pending)) {
			service.stop();
		}

		this.pending = Object.create(null) as Record<string, NSNetService>;
		this.live = Object.create(null) as Record<string, true>;
		this.resolved = Object.create(null) as Record<string, DiscoveredDaemon>;
	}
}

/** Browse `_waku._tcp` for daemons advertising on the LAN. `onChange`
 * receives the whole current set each time a service resolves or
 * disappears; the returned function stops browsing. */
export function subscribeDaemonDiscovery(
	onChange: (daemons: DiscoveredDaemon[]) => void,
): () => void {
	const delegate = DaemonBrowserDelegate.initWithCallback(onChange);
	const browser = NSNetServiceBrowser.alloc().init();
	browser.delegate = delegate;
	browser.searchForServicesOfTypeInDomain(SERVICE_TYPE, SERVICE_DOMAIN);
	return () => {
		browser.stop();
		delegate.stop();
	};
}
