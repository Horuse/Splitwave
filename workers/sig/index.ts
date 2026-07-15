import { DurableObject } from 'cloudflare:workers';

interface Env {
	ROOMS: DurableObjectNamespace<SignalingRoom>;
}

// Host stays connected; each guest `join` makes it mint a fresh offer (pre-gathered
// ones go stale as NAT mappings expire). Offer/answer/candidates relayed per joinId.
export class SignalingRoom extends DurableObject<Env> {
	async fetch(request: Request): Promise<Response> {
		if (request.headers.get('Upgrade') !== 'websocket') {
			return new Response('WebSocket upgrade required', { status: 426 });
		}

		const url = new URL(request.url);
		const role = (url.searchParams.get('role') ?? 'guest') as 'host' | 'guest';
		const passwordHash = url.searchParams.get('passwordHash') ?? '';

		const pair = new WebSocketPair();
		const [client, server] = Object.values(pair);

		if (role === 'host') {
			await this.ctx.storage.put('passwordHash', passwordHash);
			this.ctx.acceptWebSocket(server, ['host']);
		} else {
			const joinId = crypto.randomUUID();
			this.ctx.acceptWebSocket(server, ['guest', joinId]);
			const stored = (await this.ctx.storage.get<string>('passwordHash')) ?? '';
			const host = this.ctx.getWebSockets('host')[0];
			if (!host) {
				server.send(JSON.stringify({ type: 'error', reason: 'no-host' }));
				server.close(1000);
			} else if (stored && passwordHash !== stored) {
				server.send(JSON.stringify({ type: 'error', reason: 'password' }));
				server.close(1000);
			} else {
				host.send(JSON.stringify({ type: 'join', joinId }));
			}
		}

		return new Response(null, { status: 101, webSocket: client });
	}

	async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
		if (typeof message !== 'string') return;
		let msg: { type?: string; joinId?: string; peerId?: string; sdp?: string; candidate?: string };
		try {
			msg = JSON.parse(message);
		} catch {
			return;
		}

		const tags = this.ctx.getTags(ws);
		if (tags[0] === 'host') {
			if ((msg.type === 'offer' || msg.type === 'candidate') && msg.joinId) {
				for (const guest of this.ctx.getWebSockets(msg.joinId)) {
					guest.send(JSON.stringify(msg));
				}
			}
		} else {
			if (msg.type === 'answer' || msg.type === 'candidate') {
				msg.joinId = tags[1];
				for (const host of this.ctx.getWebSockets('host')) {
					host.send(JSON.stringify(msg));
				}
			}
		}
	}

	async webSocketClose(ws: WebSocket, _code: number, _reason: string): Promise<void> {
		// Room dies with its host; a reconnecting host re-registers the hash.
		if (this.ctx.getTags(ws)[0] === 'host') {
			await this.ctx.storage.deleteAll();
		}
	}

	async webSocketError(_ws: WebSocket, _error: unknown): Promise<void> {}
}

export default {
	async fetch(request: Request, env: Env): Promise<Response> {
		const url = new URL(request.url);
		const match = url.pathname.match(/^\/ws\/([A-Z0-9]{6})$/i);
		if (!match) return new Response('Not found', { status: 404 });

		const id = env.ROOMS.idFromName(match[1].toUpperCase());
		return env.ROOMS.get(id).fetch(request);
	}
} satisfies ExportedHandler<Env>;
