import { DurableObject } from 'cloudflare:workers';

interface Env {
	ROOMS: DurableObjectNamespace<SignalingRoom>;
}

export class SignalingRoom extends DurableObject<Env> {
	async fetch(request: Request): Promise<Response> {
		if (request.headers.get('Upgrade') !== 'websocket') {
			return new Response('WebSocket upgrade required', { status: 426 });
		}

		const url = new URL(request.url);
		const role = (url.searchParams.get('role') ?? 'guest') as 'host' | 'guest';
		const peerId = url.searchParams.get('peerId') ?? '';

		const pair = new WebSocketPair();
		const [client, server] = Object.values(pair);
		this.ctx.acceptWebSocket(server, [role, peerId]);

		if (role === 'guest') {
			const offer = await this.ctx.storage.get<string>('offer');
			const hostPeerId = await this.ctx.storage.get<string>('hostPeerId');
			if (offer && hostPeerId) {
				server.send(JSON.stringify({ type: 'offer', peerId: hostPeerId, sdp: offer }));
			}
		}

		return new Response(null, { status: 101, webSocket: client });
	}

	async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
		if (typeof message !== 'string') return;
		let msg: { type?: string; peerId?: string; sdp?: string };
		try {
			msg = JSON.parse(message);
		} catch {
			return;
		}

		const tags = this.ctx.getTags(ws);
		const role = tags[0];

		if (role === 'host' && msg.type === 'offer' && msg.sdp && msg.peerId) {
			await this.ctx.storage.put('offer', msg.sdp);
			await this.ctx.storage.put('hostPeerId', msg.peerId);
			for (const guestWs of this.ctx.getWebSockets('guest')) {
				guestWs.send(JSON.stringify({ type: 'offer', peerId: msg.peerId, sdp: msg.sdp }));
			}
		} else if (role === 'guest' && msg.type === 'answer' && msg.sdp && msg.peerId) {
			for (const hostWs of this.ctx.getWebSockets('host')) {
				hostWs.send(JSON.stringify({ type: 'answer', peerId: msg.peerId, sdp: msg.sdp }));
			}
		}
	}

	async webSocketClose(_ws: WebSocket, _code: number, _reason: string): Promise<void> {}
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
