<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';

	interface Session {
		pid: number;
		session_id: string;
		cwd: string;
		started_at: number;
		status: string;
		activity: string | null;
		source: string;
	}

	interface ProjectGroup {
		cwd: string;
		display_name: string;
		sessions: Session[];
	}

	let groups: ProjectGroup[] = $state([]);
	let lastScan = $state('');
	let clock = $state('');
	let booted = $state(false);

	onMount(async () => {
		// Boot sequence
		setTimeout(() => { booted = true; }, 100);

		// Live clock
		const tick = () => {
			const now = new Date();
			clock = now.toLocaleTimeString('en-US', { hour12: false });
		};
		tick();
		const clockInterval = setInterval(tick, 1000);

		try {
			groups = await invoke('get_session_groups');
			lastScan = new Date().toLocaleTimeString('en-US', { hour12: false });
		} catch (e) {
			console.error('Failed to scan sessions:', e);
		}

		await listen<ProjectGroup[]>('sessions-changed', (event) => {
			groups = event.payload;
			lastScan = new Date().toLocaleTimeString('en-US', { hour12: false });
		});

		return () => clearInterval(clockInterval);
	});

	function statusMeta(status: string) {
		switch (status) {
			case 'Working':
				return { label: 'ACTIVE', color: 'var(--color-status-active)', dim: 'var(--color-status-active-dim)', glyph: '▸' };
			case 'WaitingInput':
				return { label: 'AWAITING', color: 'var(--color-status-waiting)', dim: 'var(--color-status-waiting-dim)', glyph: '◈' };
			case 'Idle':
				return { label: 'IDLE', color: 'var(--color-status-idle)', dim: 'var(--color-status-idle-dim)', glyph: '◦' };
			default:
				return { label: 'STANDBY', color: 'var(--color-status-unknown)', dim: 'var(--color-status-unknown-dim)', glyph: '·' };
		}
	}

	async function focusSession(session: Session) {
		try {
			await invoke('focus_session', { pid: session.pid });
		} catch (e) {
			console.error('Failed to focus session:', e);
		}
	}

	function elapsed(startedAt: number): string {
		const diff = Math.floor((Date.now() - startedAt) / 1000);
		const h = Math.floor(diff / 3600);
		const m = Math.floor((diff % 3600) / 60);
		const s = diff % 60;
		if (h > 0) return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
		return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
	}

	function shortPath(cwd: string): string {
		return cwd.replace(/^\/Users\/[^/]+\//, '~/');
	}

	const totalSessions = $derived(groups.reduce((sum, g) => sum + g.sessions.length, 0));
	const activeCount = $derived(
		groups.reduce((sum, g) => sum + g.sessions.filter(s => s.status === 'Working').length, 0)
	);
	const waitingCount = $derived(
		groups.reduce((sum, g) => sum + g.sessions.filter(s => s.status === 'WaitingInput').length, 0)
	);
</script>

<div class="observatory" class:booted style="animation: flicker 8s ease-in-out infinite;">
	<!-- Scan beam -->
	<div class="scan-beam"></div>

	<!-- ═══ Top Bar ═══ -->
	<header class="top-bar">
		<div class="top-bar-left">
			<span class="observatory-title">OBSERVATORY</span>
			<span class="top-bar-divider">│</span>
			<span class="top-bar-meta">SYS.MONITOR v0.1</span>
		</div>
		<div class="top-bar-right">
			<span class="top-bar-meta">{clock}</span>
		</div>
	</header>

	<!-- ═══ Status Strip ═══ -->
	<div class="status-strip">
		<div class="status-strip-cell">
			<span class="strip-label">AGENTS</span>
			<span class="strip-value">{totalSessions}</span>
		</div>
		<div class="status-strip-cell">
			<span class="strip-label">ACTIVE</span>
			<span class="strip-value" style="color: var(--color-status-active);">{activeCount}</span>
		</div>
		<div class="status-strip-cell" class:urgent-cell={waitingCount > 0}>
			<span class="strip-label">AWAITING</span>
			<span class="strip-value" style="color: {waitingCount > 0 ? 'var(--color-status-waiting)' : 'var(--color-text-dim)'};">{waitingCount}</span>
		</div>
		<div class="status-strip-cell">
			<span class="strip-label">SCAN</span>
			<span class="strip-value">{lastScan || '—'}</span>
		</div>
	</div>

	<!-- ═══ Main Grid ═══ -->
	<main class="main-area">
		{#if groups.length === 0}
			<div class="empty-state">
				<div class="empty-glyph">◇</div>
				<p class="empty-text">NO ACTIVE SESSIONS</p>
				<p class="empty-sub">Scanning ~/.claude/sessions/</p>
			</div>
		{:else}
			{#each groups as group, gi}
				<section class="project-block" style="animation-delay: {gi * 60}ms;">
					<!-- Project Header -->
					<div class="project-header">
						<div class="project-header-left">
							<span class="project-marker">■</span>
							<span class="project-name">{group.display_name.toUpperCase()}</span>
							<span class="project-count">{group.sessions.length}</span>
						</div>
						<span class="project-path">{shortPath(group.cwd)}</span>
					</div>

					<!-- Session Rows -->
					<div class="session-list">
						{#each group.sessions as session, si}
							{@const meta = statusMeta(session.status)}
							<button
								class="session-row"
								class:session-waiting={session.status === 'WaitingInput'}
								style="
									--row-color: {meta.color};
									--row-dim: {meta.dim};
									animation-delay: {(gi * 60) + (si * 40)}ms;
								"
								onclick={() => focusSession(session)}
							>
								<!-- Status indicator -->
								<div class="session-indicator">
									<span class="indicator-glyph" style="color: {meta.color};">{meta.glyph}</span>
								</div>

								<!-- Core info -->
								<div class="session-core">
									<div class="session-id-row">
										<span class="session-source">{session.source}</span>
										<span class="session-pid">:{session.pid}</span>
									</div>
									{#if session.activity}
										<p class="session-activity">{session.activity}</p>
									{/if}
								</div>

								<!-- Status tag -->
								<div class="session-status-tag" style="
									color: {meta.color};
									border-color: {meta.color}30;
									background: {meta.dim};
								">
									{meta.label}
								</div>

								<!-- Elapsed -->
								<div class="session-elapsed">
									{elapsed(session.started_at)}
								</div>
							</button>
						{/each}
					</div>
				</section>
			{/each}
		{/if}
	</main>

	<!-- ═══ Bottom Bar ═══ -->
	<footer class="bottom-bar">
		<span class="bottom-meta">◈ CLICK TO FOCUS</span>
		<span class="bottom-meta">POLL: 3s</span>
		<span class="bottom-meta">macOS {'{'}KERN_PROCARGS2{'}'}</span>
	</footer>
</div>

<style>
	/* ═══════════════════════════════════════════
	   OBSERVATORY — Command Center
	   Alien futuristic × brutalist terminal
	   ═══════════════════════════════════════════ */

	.observatory {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background: var(--color-bg);
		user-select: none;
		position: relative;
		overflow: hidden;
		opacity: 0;
		transition: opacity 0.6s ease;
	}
	.observatory.booted { opacity: 1; }

	/* Ambient corner vignette */
	.observatory::before {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		background: radial-gradient(ellipse at 50% 0%, transparent 50%, var(--color-void) 100%);
		z-index: 1;
	}

	/* Slow scanning beam */
	.scan-beam {
		position: absolute;
		top: 0;
		left: 0;
		right: 0;
		height: 1px;
		background: linear-gradient(90deg, transparent 0%, var(--color-accent) 50%, transparent 100%);
		opacity: 0.08;
		z-index: 10;
		animation: scan 12s linear infinite;
		pointer-events: none;
	}

	/* ── Top Bar ── */
	.top-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 20px;
		border-bottom: 1px solid var(--color-border);
		background: var(--color-surface);
		position: relative;
		z-index: 2;
	}
	.top-bar::after {
		content: '';
		position: absolute;
		bottom: -1px;
		left: 0;
		right: 0;
		height: 1px;
		background: linear-gradient(90deg, transparent, var(--color-accent)20, transparent);
	}
	.top-bar-left, .top-bar-right {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.observatory-title {
		font-family: var(--font-display);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.25em;
		color: var(--color-accent);
	}
	.top-bar-divider {
		color: var(--color-text-ghost);
		font-size: 11px;
	}
	.top-bar-meta {
		font-size: 11px;
		color: var(--color-text-dim);
		letter-spacing: 0.05em;
	}

	/* ── Status Strip ── */
	.status-strip {
		display: flex;
		border-bottom: 1px solid var(--color-border);
		background: var(--color-void);
		position: relative;
		z-index: 2;
	}
	.status-strip-cell {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 8px 12px;
		border-right: 1px solid var(--color-border);
	}
	.status-strip-cell:last-child {
		border-right: none;
	}
	.strip-label {
		font-size: 9px;
		letter-spacing: 0.15em;
		color: var(--color-text-dim);
		font-family: var(--font-display);
	}
	.strip-value {
		font-size: 13px;
		color: var(--color-text-secondary);
		font-variant-numeric: tabular-nums;
	}
	.urgent-cell {
		background: var(--color-urgent-dim);
		animation: urgent-beacon 2s ease-in-out infinite;
	}

	/* ── Main Area ── */
	.main-area {
		flex: 1;
		overflow-y: auto;
		padding: 16px 20px;
		display: flex;
		flex-direction: column;
		gap: 20px;
		position: relative;
		z-index: 2;
	}

	/* ── Empty State ── */
	.empty-state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
	}
	.empty-glyph {
		font-size: 32px;
		color: var(--color-text-ghost);
		animation: blink 3s ease-in-out infinite;
	}
	.empty-text {
		font-family: var(--font-display);
		font-size: 11px;
		letter-spacing: 0.2em;
		color: var(--color-text-dim);
	}
	.empty-sub {
		font-size: 10px;
		color: var(--color-text-ghost);
	}

	/* ── Project Block ── */
	.project-block {
		animation: boot-in 0.3s ease-out both;
	}
	.project-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0 4px 8px;
	}
	.project-header-left {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.project-marker {
		font-size: 6px;
		color: var(--color-accent);
	}
	.project-name {
		font-family: var(--font-display);
		font-size: 10px;
		font-weight: 500;
		letter-spacing: 0.2em;
		color: var(--color-text-secondary);
	}
	.project-count {
		font-size: 9px;
		color: var(--color-text-ghost);
		border: 1px solid var(--color-border);
		padding: 1px 5px;
		line-height: 1.2;
	}
	.project-path {
		font-size: 10px;
		color: var(--color-text-ghost);
		letter-spacing: 0.02em;
	}

	/* ── Session List ── */
	.session-list {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	/* ── Session Row ── */
	.session-row {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px 12px;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-left: 2px solid var(--row-color, var(--color-text-ghost));
		cursor: pointer;
		text-align: left;
		width: 100%;
		transition: all 0.12s ease;
		animation: boot-in 0.3s ease-out both;
		position: relative;
	}
	.session-row:hover {
		background: var(--color-surface-hover);
		border-color: var(--color-border-bright);
		border-left-color: var(--row-color);
	}
	.session-row:hover .session-source {
		color: var(--color-text-primary);
	}

	/* Waiting row urgency */
	.session-waiting {
		background: var(--color-urgent-dim);
		border-color: var(--color-urgent)30;
	}
	.session-waiting::after {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		box-shadow: inset 0 0 20px var(--color-urgent-dim);
		animation: urgent-beacon 2s ease-in-out infinite;
	}

	/* ── Session Parts ── */
	.session-indicator {
		width: 20px;
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
	}
	.indicator-glyph {
		font-size: 14px;
		line-height: 1;
	}

	.session-core {
		flex: 1;
		min-width: 0;
	}
	.session-id-row {
		display: flex;
		align-items: baseline;
		gap: 2px;
	}
	.session-source {
		font-size: 12px;
		color: var(--color-text-secondary);
		transition: color 0.12s ease;
	}
	.session-pid {
		font-size: 11px;
		color: var(--color-text-ghost);
	}
	.session-activity {
		font-size: 10px;
		color: var(--color-text-dim);
		margin-top: 2px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.session-status-tag {
		font-family: var(--font-display);
		font-size: 8px;
		font-weight: 500;
		letter-spacing: 0.18em;
		padding: 3px 8px;
		border: 1px solid;
		flex-shrink: 0;
	}

	.session-elapsed {
		font-size: 11px;
		color: var(--color-text-dim);
		font-variant-numeric: tabular-nums;
		width: 60px;
		text-align: right;
		flex-shrink: 0;
	}

	/* ── Bottom Bar ── */
	.bottom-bar {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 24px;
		padding: 6px 20px;
		border-top: 1px solid var(--color-border);
		background: var(--color-void);
		position: relative;
		z-index: 2;
	}
	.bottom-meta {
		font-size: 9px;
		letter-spacing: 0.1em;
		color: var(--color-text-ghost);
	}
</style>
