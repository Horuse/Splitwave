<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import type * as MatterTypes from 'matter-js';
	import type { Picture } from '@sveltejs/enhanced-img';

	const NODES = [
		'microphone',
		'system_audio',
		'app_audio',
		'audio_file',
		'gain',
		'mute',
		'channel_balance',
		'saturator',
		'eq',
		'limiter',
		'compressor',
		'noise_gate',
		'noise_suppressor',
		'delay',
		'reverb',
		'declick',
		'de_esser',
		'plugin',
		'level_meter',
		'lufs_meter',
		'waveform',
		'spectrum',
		'web_rtc_collaborator',
		'net_receiver',
		'net_sender',
		'speaker',
		'file_recording'
	];

	const NODE_IMAGES = import.meta.glob('../../../static/nodes/*.webp', {
		eager: true,
		query: { enhanced: true, format: 'avif;webp', w: '800;400' },
		import: 'default'
	}) as Record<string, Picture>;

	const imageFor = (id: string) => NODE_IMAGES[`../../../static/nodes/${id}.webp`];
	const ORIGINAL_DIMENSIONS: Record<string, [number, number]> = {
		app_audio: [1160, 604],
		audio_file: [1160, 948],
		channel_balance: [936, 904],
		compressor: [968, 2228],
		de_esser: [936, 1252],
		declick: [936, 832],
		delay: [968, 1344],
		eq: [1280, 1392],
		file_recording: [1160, 1312],
		gain: [904, 440],
		level_meter: [540, 1636],
		limiter: [936, 1448],
		lufs_meter: [1680, 1620],
		microphone: [936, 840],
		mute: [776, 400],
		net_receiver: [840, 620],
		net_sender: [904, 1376],
		noise_gate: [968, 1832],
		noise_suppressor: [968, 1636],
		plugin: [1032, 352],
		reverb: [968, 1496],
		saturator: [936, 1096],
		speaker: [936, 744],
		spectrum: [1280, 640],
		system_audio: [1160, 672],
		waveform: [1280, 640],
		web_rtc_collaborator: [968, 1668]
	};

	const label = (id: string) => id.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());

	const SCALE = 1 / 5;
	const PAD = 5;

	const HEIGHT = 900;
	const ATTRACT = 0.0003;
	const STAGGER_MS = 200;
	const FLY_IN_SPEED = 10;
	const SPAWN_OFFSET = 50;

	const smallWebpFor = (id: string) => {
		const displayWidth = ORIGINAL_DIMENSIONS[id][0] * SCALE;
		const candidates = imageFor(id)
			.sources.webp.split(',')
			.map((entry) => {
				const [url, width] = entry.trim().split(/\s+/);
				return { url, width: Number.parseInt(width) };
			});
		return (candidates.find(({ width }) => width >= displayWidth * 2) ?? candidates.at(-1))!.url;
	};

	const SLIDE_MS = 1000;
	const CLOUD_QUERY = '(width >= 48rem)';

	let container: HTMLDivElement;
	let Matter: typeof MatterTypes | undefined;
	let engine: MatterTypes.Engine | undefined;
	let runner: MatterTypes.Runner | undefined;
	let bodies: { id: string; body: MatterTypes.Body; el: HTMLImageElement; w: number; h: number }[] =
		[];
	let raf = 0;
	let resizeObs: ResizeObserver | undefined;
	let centre = { x: 0, y: HEIGHT / 2 };
	const flyInTimers: ReturnType<typeof setTimeout>[] = [];

	let slide = $state(0);
	let slideTimer: ReturnType<typeof setInterval> | undefined;

	async function measure(
		id: string
	): Promise<{ id: string; el: HTMLImageElement; w: number; h: number }> {
		const el = document.createElement('img');
		el.src = smallWebpFor(id);
		el.alt = `${label(id)} node — Splitwave audio routing`;
		el.title = `${label(id)} node`;
		el.draggable = false;
		el.className =
			'absolute top-0 left-0 select-none rounded-xl shadow-lg shadow-black/30 will-change-transform pointer-events-none';
		await el.decode();
		const [naturalWidth, naturalHeight] = ORIGINAL_DIMENSIONS[id];
		return {
			id,
			el,
			w: Math.round(naturalWidth * SCALE),
			h: Math.round(naturalHeight * SCALE)
		};
	}

	async function startCloud() {
		Matter = (await import('matter-js')).default as unknown as typeof MatterTypes;
		const M = Matter;
		const measured = await Promise.all(NODES.map(measure));
		if (!container) return;

		const width = container.clientWidth;
		const height = HEIGHT;
		centre = { x: width / 2, y: height / 2 };

		engine = M.Engine.create({ gravity: { x: 0, y: 0, scale: 0 } });
		runner = M.Runner.create();

		let spawnAngle = 0;
		const GOLDEN_ANGLE = 2.39996;

		const spawnOne = (sz: { id: string; el: HTMLImageElement; w: number; h: number }) => {
			if (!engine || !container) return;
			const w = container.clientWidth;

			const radius = Math.max(w, HEIGHT) / 2 + SPAWN_OFFSET;
			const x = centre.x + Math.cos(spawnAngle) * radius;
			const y = centre.y + Math.sin(spawnAngle) * radius;
			spawnAngle += GOLDEN_ANGLE;

			const body = M.Bodies.rectangle(x, y, sz.w + PAD * 2, sz.h + PAD * 2, {
				chamfer: { radius: 16 },
				restitution: 0.4,
				friction: 0.1,
				frictionAir: 0.03,
				density: 0.0015,
				inertia: Infinity,
				inverseInertia: 0
			});

			// Для більш органічного прильоту даємо невеликий розкид цілі,
			// але всі вони ціляться в район центру екрану
			const targetX = centre.x + (Math.random() - 0.5) * (w * 0.3);
			const targetY = centre.y + (Math.random() - 0.5) * 150;

			const dx = targetX - x;
			const dy = targetY - y;
			const dist = Math.max(1, Math.hypot(dx, dy));

			M.Body.setVelocity(body, {
				x: (dx / dist) * FLY_IN_SPEED,
				y: (dy / dist) * FLY_IN_SPEED
			});

			sz.el.style.width = `${sz.w}px`;
			sz.el.style.height = `${sz.h}px`;
			container.appendChild(sz.el);

			bodies.push({ id: sz.id, body, el: sz.el, w: sz.w, h: sz.h });
			M.World.add(engine.world, body);
		};

		const order = [...measured].sort((a, b) => b.w * b.h - a.w * a.h);
		order.forEach((sz, i) => {
			flyInTimers.push(setTimeout(() => spawnOne(sz), i * STAGGER_MS));
		});

		const DEADZONE = 20; // Зменшили мертву зону, щоб центр був щільніше заповнений
		M.Events.on(engine, 'beforeUpdate', () => {
			for (const { body } of bodies) {
				const dx = centre.x - body.position.x;
				const dy = centre.y - body.position.y;
				const d = Math.hypot(dx, dy);

				if (d < DEADZONE) continue;
				const f = ATTRACT * body.mass;

				// МАГІЯ ТУТ: Еліптичне тяжіння
				// По горизонталі (x) притягуємо слабше (0.5), тому вони розтікаються в ширину
				// По вертикалі (y) притягуємо сильніше (1.5), тому вони щільно стискаються по висоті
				M.Body.applyForce(body, body.position, {
					x: (dx / d) * f * 0.5,
					y: (dy / d) * f * 1.5
				});
			}
		});

		const mouse = M.Mouse.create(container);
		const mouseConstraint = M.MouseConstraint.create(engine, {
			mouse,
			constraint: { stiffness: 0.2, damping: 0.1, render: { visible: false } }
		});
		M.World.add(engine.world, mouseConstraint);

		const mouseAny = mouse as unknown as { element: HTMLElement; mousewheel: EventListener };
		mouseAny.element.removeEventListener('wheel', mouseAny.mousewheel);

		M.Runner.run(runner, engine);

		const tick = () => {
			for (const { body, el, w, h } of bodies) {
				const x = body.position.x - w / 2;
				const y = body.position.y - h / 2;
				el.style.transform = `translate(${x}px, ${y}px)`;
			}
			raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);

		resizeObs = new ResizeObserver(() => {
			if (!container) return;
			centre = { x: container.clientWidth / 2, y: HEIGHT / 2 };
		});
		resizeObs.observe(container);
	}

	onMount(() => {
		if (window.matchMedia(CLOUD_QUERY).matches) {
			startCloud();
			return;
		}
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
		slideTimer = setInterval(() => (slide = (slide + 1) % NODES.length), SLIDE_MS);
	});

	onDestroy(() => {
		clearInterval(slideTimer);
		if (!Matter || !engine) return;
		for (const t of flyInTimers) clearTimeout(t);
		flyInTimers.length = 0;
		cancelAnimationFrame(raf);
		resizeObs?.disconnect();
		if (runner) Matter.Runner.stop(runner);
		Matter.World.clear(engine.world, false);
		Matter.Engine.clear(engine);
		for (const { el } of bodies) el.remove();
		bodies = [];
	});
</script>

<section class="w-full px-6 py-16">
	<div class="mb-6 text-center">
		<h2 class="text-2xl font-semibold tracking-tight">
			Twenty-seven building blocks for routing audio between apps
		</h2>
	</div>

	<div class="md:hidden">
		<div class="overflow-hidden">
			<div
				class="flex transition-transform duration-500 ease-out"
				style="transform: translateX(-{slide * 100}%);"
			>
				{#each NODES as id, i (id)}
					<div class="flex w-full shrink-0 items-center justify-center">
						<enhanced:img
							src={imageFor(id)}
							alt="{label(id)} node — Splitwave audio routing"
							title="{label(id)} node"
							class="max-h-56 w-auto rounded-xl shadow-lg shadow-black/30"
							loading={i < 2 ? 'eager' : 'lazy'}
							draggable="false"
						/>
					</div>
				{/each}
			</div>
		</div>
		<p class="mt-5 text-center text-sm text-neutral-1000">{label(NODES[slide])}</p>
	</div>

	<div
		bind:this={container}
		class="relative hidden w-full cursor-grab overflow-hidden active:cursor-grabbing md:block"
		style="height: {HEIGHT}px;"
	>
		<div
			class="pointer-events-none absolute inset-0 z-10"
			style="background:
             linear-gradient(to bottom, var(--color-background) 0%, transparent 6rem, transparent calc(100% - 6rem), var(--color-background) 100%),
             linear-gradient(to right, var(--color-background) 0%, transparent 6rem, transparent calc(100% - 6rem), var(--color-background) 100%);"
		></div>
	</div>
</section>
