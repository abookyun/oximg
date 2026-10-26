// The released oximg server image behind a Worker. Each container
// instance is owned by one Durable Object; a URL path always maps to the
// same one, so identical requests still meet in one process and the
// server's request coalescing keeps working.
import { Container, getContainer } from "@cloudflare/containers";

export class OximgContainer extends Container {
	defaultPort = 8081;
	// Measured: a stopped instance answers its first request in 0.9-1.2 s.
	// Warning: with @cloudflare/containers 0.3.7, requests the client
	// aborted mid-flight were observed to keep the instance awake past
	// this (see docs/deploy-cloudflare.md, "Sleep").
	sleepAfter = "3m";

	constructor(ctx, env) {
		super(ctx, env);
		this.envVars = {
			OXIMG_SOURCE_BASE_URL: env.OXIMG_SOURCE_BASE_URL,
			...(env.OXIMG_METRICS ? { OXIMG_METRICS: env.OXIMG_METRICS } : {}),
		};
	}
}

// FNV-1a, then murmur3's fmix32: stable across deploys. FNV-1a alone
// put /resize/{700,711,…,799}/800/1mp.jpg@webp all in one of two
// buckets; its low bits track the input's structure.
function pathHash(s) {
	let h = 0x811c9dc5;
	for (let i = 0; i < s.length; i++) {
		h ^= s.charCodeAt(i);
		h = Math.imul(h, 0x01000193);
	}
	h ^= h >>> 16;
	h = Math.imul(h, 0x85ebca6b);
	h ^= h >>> 13;
	h = Math.imul(h, 0xc2b2ae35);
	h ^= h >>> 16;
	return h >>> 0;
}

export default {
	async fetch(req, env) {
		const n = Math.max(1, Number(env.INSTANCES) || 1);
		const path = new URL(req.url).pathname;
		return getContainer(env.OXIMG, `oximg-${pathHash(path) % n}`).fetch(req);
	},
};
