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
		slug: string | null;
		model: string | null;
		context_used: number | null;
		context_max: number | null;
		git_branch: string | null;
		last_message: string | null;
	}

	interface ProjectGroup {
		cwd: string;
		display_name: string;
		sessions: Session[];
	}

	const THEMES = ['nightfall', 'fieldcom', 'warmdesk'] as const;
	type Theme = typeof THEMES[number];

	let groups: ProjectGroup[] = $state([]);
	let lastScan = $state('');
	let clock = $state('');
	let booted = $state(false);
	let theme: Theme = $state((localStorage.getItem('observatory-theme') as Theme) || 'nightfall');

	$effect(() => {
		document.documentElement.setAttribute('data-theme', theme);
		localStorage.setItem('observatory-theme', theme);
	});

	onMount(async () => {
		document.documentElement.setAttribute('data-theme', theme);
		setTimeout(() => { booted = true; }, 100);

		const tick = () => {
			clock = new Date().toLocaleTimeString('en-US', { hour12: false });
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

	function cycleTheme() {
		const idx = THEMES.indexOf(theme);
		theme = THEMES[(idx + 1) % THEMES.length];
	}

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

	function contextPct(session: Session): number | null {
		if (!session.context_used || !session.context_max) return null;
		return Math.round((session.context_used / session.context_max) * 100);
	}

	function contextColor(pct: number): string {
		if (pct >= 80) return 'var(--color-urgent)';
		if (pct >= 60) return 'var(--color-status-waiting)';
		return 'var(--color-status-active)';
	}

	function sessionLabel(session: Session): string {
		return session.slug || `${session.source}:${session.pid}`;
	}

	const totalSessions = $derived(groups.reduce((sum, g) => sum + g.sessions.length, 0));
	const activeCount = $derived(
		groups.reduce((sum, g) => sum + g.sessions.filter(s => s.status === 'Working').length, 0)
	);
	const waitingCount = $derived(
		groups.reduce((sum, g) => sum + g.sessions.filter(s => s.status === 'WaitingInput').length, 0)
	);
</script>

<div class="observatory" class:booted>
	<div class="scan-beam"></div>

	<header class="top-bar">
		<div class="top-bar-left">
			<span class="observatory-title">OBSERVATORY</span>
			<span class="top-bar-divider">│</span>
			<span class="top-bar-meta">v0.3</span>
		</div>
		<div class="top-bar-right">
			<button class="theme-btn" onclick={cycleTheme} title="Switch theme">
				{theme === 'nightfall' ? '◑' : theme === 'fieldcom' ? '◐' : '○'}
			</button>
			<span class="top-bar-meta">{clock}</span>
		</div>
	</header>

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
					<div class="project-header">
						<div class="project-header-left">
							<span class="project-marker">■</span>
							<span class="project-name">{group.display_name.toUpperCase()}</span>
							<span class="project-count">{group.sessions.length}</span>
						</div>
						<span class="project-path">{shortPath(group.cwd)}</span>
					</div>

					<div class="session-list">
						{#each group.sessions as session, si}
							{@const meta = statusMeta(session.status)}
							{@const pct = contextPct(session)}
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
								<div class="session-indicator">
									<span class="indicator-glyph" style="color: {meta.color};">{meta.glyph}</span>
								</div>

								<div class="session-core">
									<div class="session-id-row">
										<span class="session-slug">{sessionLabel(session)}</span>
										{#if session.activity}
											<span class="session-activity-inline">— {session.activity}</span>
										{/if}
									</div>

									<div class="session-meta-row">
										{#if session.model}
											<span class="session-model">{session.model}</span>
										{/if}
										{#if pct !== null}
											<div class="context-bar-wrap">
												<div class="context-bar-fill" style="width: {pct}%; background: {contextColor(pct)};"></div>
											</div>
											<span class="context-label" style="color: {contextColor(pct)};">{pct}%</span>
										{/if}
										{#if session.git_branch}
											<span class="session-branch">{session.git_branch}</span>
										{/if}
										<span class="session-source-small">{session.source}</span>
									</div>

									{#if session.last_message}
										<p class="session-message">{session.last_message}</p>
									{/if}
								</div>

								<div class="session-status-tag" style="
									color: {meta.color};
									border-color: {meta.color}40;
									background: {meta.dim};
								">
									{meta.label}
								</div>

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

	<footer class="bottom-bar">
		<span class="bottom-meta">◈ CLICK TO FOCUS</span>
		<span class="bottom-meta">POLL 10s</span>
	</footer>
</div>

<style>
	/* ── Observatory Shell ── */
	.observatory {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background: var(--color-bg);
		user-select: none;
		position: relative;
		overflow: hidden;
		opacity: 0;
		transition: opacity 0.4s ease, background-color 0.3s ease, color 0.3s ease;
	}
	.observatory.booted {
		opacity: 1;
		animation: var(--flicker);
	}

	.observatory::before {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		background: radial-gradient(ellipse at 50% 0%, transparent 60%, var(--color-void) 100%);
		z-index: 1;
	}

	.scan-beam {
		position: absolute;
		top: 0; left: 0; right: 0;
		height: 1px;
		background: linear-gradient(90deg, transparent 0%, var(--color-accent) 50%, transparent 100%);
		opacity: 0.06;
		z-index: 10;
		animation: scan 14s linear infinite;
		pointer-events: none;
	}

	/* ── Top Bar ── */
	.top-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 20px;
		border-bottom: 1px var(--border-style) var(--color-border);
		background: var(--color-surface);
		position: relative;
		z-index: 2;
		transition: background-color 0.3s ease;
	}
	.top-bar::after {
		content: '';
		position: absolute;
		bottom: -1px; left: 0; right: 0;
		height: 1px;
		background: linear-gradient(90deg, transparent, var(--color-accent)15, transparent);
	}
	.top-bar-left, .top-bar-right {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.observatory-title {
		font-family: var(--font-display);
		font-size: 12px;
		font-weight: 600;
		letter-spacing: 0.2em;
		color: var(--color-accent);
	}
	.top-bar-divider {
		color: var(--color-text-ghost);
		font-size: 12px;
	}
	.top-bar-meta {
		font-family: var(--font-data);
		font-size: 11px;
		color: var(--color-text-dim);
		letter-spacing: 0.03em;
	}
	.theme-btn {
		background: none;
		border: 1px var(--border-style) var(--color-border);
		color: var(--color-text-secondary);
		font-size: 14px;
		width: 28px;
		height: 28px;
		display: flex;
		align-items: center;
		justify-content: center;
		cursor: pointer;
		border-radius: var(--radius);
		transition: all 0.15s ease;
	}
	.theme-btn:hover {
		border-color: var(--color-accent);
		color: var(--color-accent);
		background: var(--color-surface-hover);
	}

	/* ── Status Strip ── */
	.status-strip {
		display: flex;
		border-bottom: 1px var(--border-style) var(--color-border);
		background: var(--color-void);
		position: relative;
		z-index: 2;
		transition: background-color 0.3s ease;
	}
	.status-strip-cell {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 7px 12px;
		border-right: 1px var(--border-style) var(--color-border);
	}
	.status-strip-cell:last-child { border-right: none; }
	.strip-label {
		font-size: 9px;
		letter-spacing: 0.15em;
		color: var(--color-text-dim);
		font-family: var(--font-display);
	}
	.strip-value {
		font-family: var(--font-data);
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
		padding: 14px 18px;
		display: flex;
		flex-direction: column;
		gap: 18px;
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
		font-size: 28px;
		color: var(--color-text-ghost);
		animation: blink 3s ease-in-out infinite;
	}
	.empty-text {
		font-family: var(--font-display);
		font-size: 12px;
		letter-spacing: 0.15em;
		color: var(--color-text-dim);
	}
	.empty-sub {
		font-family: var(--font-data);
		font-size: 11px;
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
		padding: 0 2px 6px;
	}
	.project-header-left {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.project-marker {
		font-size: 7px;
		color: var(--color-accent);
	}
	.project-name {
		font-family: var(--font-display);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.15em;
		color: var(--color-text-secondary);
	}
	.project-count {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-text-dim);
		border: 1px var(--border-style) var(--color-border);
		padding: 1px 6px;
		line-height: 1.3;
		border-radius: var(--radius);
	}
	.project-path {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-text-ghost);
	}

	/* ── Session List ── */
	.session-list {
		display: flex;
		flex-direction: column;
		gap: 3px;
	}

	/* ── Session Row ── */
	.session-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		padding: 10px 14px;
		background: var(--color-surface);
		border: 1px var(--border-style) var(--color-border);
		border-left: var(--row-border-left-width) var(--border-style) var(--row-color, var(--color-text-ghost));
		border-radius: var(--radius);
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
	.session-row:hover .session-slug {
		color: var(--color-text-primary);
	}

	.session-waiting {
		background: var(--color-urgent-dim);
		border-color: var(--color-border-bright);
	}
	.session-waiting::after {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		border-radius: var(--radius);
		box-shadow: inset 0 0 16px var(--color-urgent-dim);
		animation: urgent-beacon 2s ease-in-out infinite;
	}

	/* ── Session Parts ── */
	.session-indicator {
		width: 18px;
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
		padding-top: 1px;
	}
	.indicator-glyph {
		font-size: 13px;
		line-height: 1;
	}

	.session-core {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.session-id-row {
		display: flex;
		align-items: baseline;
		gap: 8px;
	}
	.session-slug {
		font-family: var(--font-body);
		font-size: 13px;
		font-weight: 500;
		color: var(--color-text-primary);
		transition: color 0.12s ease;
	}
	.session-activity-inline {
		font-family: var(--font-data);
		font-size: 11px;
		color: var(--color-text-dim);
	}

	.session-meta-row {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 11px;
	}
	.session-model {
		font-family: var(--font-display);
		font-size: 9px;
		font-weight: 500;
		letter-spacing: 0.1em;
		color: var(--color-text-secondary);
		text-transform: uppercase;
	}
	.context-bar-wrap {
		width: 50px;
		height: 5px;
		background: var(--color-border);
		overflow: hidden;
		flex-shrink: 0;
		border-radius: 1px;
	}
	.context-bar-fill {
		height: 100%;
		transition: width 0.3s ease;
		border-radius: 1px;
	}
	.context-label {
		font-family: var(--font-data);
		font-size: 10px;
		font-variant-numeric: tabular-nums;
	}
	.session-branch {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-text-dim);
	}
	.session-source-small {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-text-ghost);
		margin-left: auto;
	}

	.session-message {
		font-family: var(--font-body);
		font-size: 11px;
		color: var(--color-text-dim);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		margin: 0;
		line-height: 1.4;
	}

	.session-status-tag {
		font-family: var(--font-display);
		font-size: 9px;
		font-weight: 500;
		letter-spacing: 0.12em;
		padding: 3px 8px;
		border: 1px var(--border-style);
		border-radius: var(--radius);
		flex-shrink: 0;
		margin-top: 1px;
	}

	.session-elapsed {
		font-family: var(--font-data);
		font-size: 12px;
		color: var(--color-text-secondary);
		font-variant-numeric: tabular-nums;
		width: 64px;
		text-align: right;
		flex-shrink: 0;
		margin-top: 1px;
	}

	/* ── Bottom Bar ── */
	.bottom-bar {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 20px;
		padding: 5px 20px;
		border-top: 1px var(--border-style) var(--color-border);
		background: var(--color-void);
		position: relative;
		z-index: 2;
		transition: background-color 0.3s ease;
	}
	.bottom-meta {
		font-family: var(--font-data);
		font-size: 9px;
		letter-spacing: 0.08em;
		color: var(--color-text-ghost);
	}
</style>
