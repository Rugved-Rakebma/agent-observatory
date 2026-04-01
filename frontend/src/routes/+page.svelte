<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { onMount, tick } from 'svelte';

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
		tool_detail: string | null;
	}

	interface ProjectGroup {
		cwd: string;
		display_name: string;
		sessions: Session[];
	}

	interface ConversationMessage {
		role: string;
		messageType: string;
		text: string | null;
		toolName: string | null;
		toolInputSummary: string | null;
		toolResultContent: string | null;
		isError: boolean | null;
		timestamp: string | null;
	}

	interface ConversationData {
		sessionId: string;
		messages: ConversationMessage[];
		totalEntries: number;
	}

	const THEMES = ['nightfall', 'fieldcom', 'warmdesk'] as const;
	type Theme = typeof THEMES[number];

	let groups: ProjectGroup[] = $state([]);
	let lastScan = $state('');
	let clock = $state('');
	let booted = $state(false);
	let theme: Theme = $state((localStorage.getItem('observatory-theme') as Theme) || 'nightfall');

	// Conversation panel state
	let viewingSession: Session | null = $state(null);
	let conversationData: ConversationData | null = $state(null);
	let conversationLoading = $state(false);

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

		await listen<ProjectGroup[]>('sessions-changed', async (event) => {
			groups = event.payload;
			lastScan = new Date().toLocaleTimeString('en-US', { hour12: false });

			// Auto-refresh conversation if one is open
			if (viewingSession && !conversationLoading) {
				try {
					const data = await invoke<ConversationData>('get_conversation', {
						sessionId: viewingSession.session_id,
						cwd: viewingSession.cwd
					});
					const hadNew = !conversationData || data.totalEntries !== conversationData.totalEntries;
					conversationData = data;
					if (hadNew) {
						await tick();
						const el = document.querySelector('.conversation-messages');
						if (el) el.scrollTop = el.scrollHeight;
					}
				} catch {}
			}
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

	async function openConversation(session: Session) {
		viewingSession = session;
		conversationLoading = true;
		try {
			conversationData = await invoke('get_conversation', {
				sessionId: session.session_id,
				cwd: session.cwd
			});
		} catch (e) {
			console.error('Failed to load conversation:', e);
			conversationData = null;
		}
		conversationLoading = false;
		await tick();
		const el = document.querySelector('.conversation-messages');
		if (el) el.scrollTop = el.scrollHeight;
	}

	async function refreshConversation() {
		if (!viewingSession) return;
		await openConversation(viewingSession);
	}

	function closeConversation() {
		viewingSession = null;
		conversationData = null;
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

	function renderText(text: string): string {
		// Escape HTML
		let s = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
		// Code blocks: ```...```
		s = s.replace(/```[\w]*\n?([\s\S]*?)```/g, '<pre class="code-block">$1</pre>');
		// Inline code: `...`
		s = s.replace(/`([^`]+)`/g, '<code class="inline-code">$1</code>');
		// Newlines (but not inside <pre>)
		s = s.replace(/\n/g, '<br>');
		return s;
	}

	function formatTime(ts: string | null): string {
		if (!ts) return '';
		try {
			const d = new Date(ts);
			return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
		} catch {
			return '';
		}
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
			<span class="top-bar-meta">v0.5</span>
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

	<main class="main-area" class:panel-open={viewingSession}>
		<div class="session-list-pane">
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
									class:session-viewing={viewingSession?.session_id === session.session_id}
									style="
										--row-color: {meta.color};
										--row-dim: {meta.dim};
										animation-delay: {(gi * 60) + (si * 40)}ms;
									"
									onclick={() => openConversation(session)}
								>
									<div class="session-indicator">
										<span class="indicator-glyph" style="color: {meta.color};">{meta.glyph}</span>
									</div>

									<div class="session-core">
										<div class="session-id-row">
											<span class="session-slug">{sessionLabel(session)}</span>
											{#if session.tool_detail}
												<span class="session-activity-inline">— {session.tool_detail}</span>
											{:else if session.activity}
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

									<!-- svelte-ignore a11y_no_static_element_interactions -->
								<div
									class="session-focus-btn"
									role="button"
									tabindex="-1"
									onclick={(e) => { e.stopPropagation(); focusSession(session); }}
									title="Go to terminal"
								>→</div>
								</button>
							{/each}
						</div>
					</section>
				{/each}
			{/if}
		</div>

		{#if viewingSession}
			<div class="conversation-panel">
				<div class="conversation-header">
					<div class="conversation-header-left">
						<span class="conversation-slug">{sessionLabel(viewingSession)}</span>
						{#if viewingSession.model}
							<span class="conversation-model">{viewingSession.model}</span>
						{/if}
						{#if conversationData}
							<span class="conversation-count">{conversationData.totalEntries} entries</span>
						{/if}
					</div>
					<div class="conversation-header-right">
						<button class="conv-btn" onclick={refreshConversation} title="Refresh">↻</button>
						<button class="conv-btn" onclick={closeConversation} title="Close">✕</button>
					</div>
				</div>

				<div class="conversation-messages">
					{#if conversationLoading}
						<div class="conv-loading">Loading conversation...</div>
					{:else if !conversationData}
						<div class="conv-loading">No conversation data available</div>
					{:else if conversationData.messages.length === 0}
						<div class="conv-loading">Conversation is empty</div>
					{:else}
						{#each conversationData.messages as msg}
							{#if msg.messageType === 'text' && msg.role === 'user'}
								<div class="msg msg-user">
									<span class="msg-time">{formatTime(msg.timestamp)}</span>
									<div class="msg-content">{@html renderText(msg.text || '')}</div>
								</div>
							{:else if msg.messageType === 'text' && msg.role === 'assistant'}
								<div class="msg msg-assistant">
									<div class="msg-content">{@html renderText(msg.text || '')}</div>
								</div>
							{:else if msg.messageType === 'tool_use'}
								<div class="msg msg-tool-use">
									<span class="tool-badge">{msg.toolName}</span>
									<span class="tool-summary">{msg.toolInputSummary || ''}</span>
								</div>
							{:else if msg.messageType === 'tool_result'}
								<div class="msg msg-tool-result" class:error={msg.isError}>
									{#if msg.isError}
										<span class="tool-error-label">ERROR</span>
									{/if}
									<pre class="tool-output">{msg.toolResultContent || ''}</pre>
								</div>
							{:else if msg.messageType === 'thinking'}
								<details class="msg msg-thinking">
									<summary>Thinking...</summary>
									<p>{msg.text || ''}</p>
								</details>
							{:else if msg.role === 'system'}
								<div class="msg msg-system">{msg.text || ''}</div>
							{/if}
						{/each}
					{/if}
				</div>
			</div>
		{/if}
	</main>

	<footer class="bottom-bar">
		<span class="bottom-meta">CLICK TO VIEW</span>
		<span class="bottom-meta">→ GO TO TERMINAL</span>
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
	.top-bar-divider { color: var(--color-text-ghost); font-size: 12px; }
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
		width: 28px; height: 28px;
		display: flex; align-items: center; justify-content: center;
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
		display: flex; align-items: center; justify-content: center;
		gap: 8px;
		padding: 7px 12px;
		border-right: 1px var(--border-style) var(--color-border);
	}
	.status-strip-cell:last-child { border-right: none; }
	.strip-label {
		font-size: 9px; letter-spacing: 0.15em;
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

	/* ── Main Area (split pane) ── */
	.main-area {
		flex: 1;
		display: flex;
		flex-direction: column;
		position: relative;
		z-index: 2;
		overflow: hidden;
	}
	.main-area.panel-open {
		flex-direction: row;
	}

	.session-list-pane {
		flex: 1;
		overflow-y: auto;
		padding: 14px 18px;
		display: flex;
		flex-direction: column;
		gap: 18px;
	}
	.main-area.panel-open .session-list-pane {
		flex: 0 0 42%;
		border-right: 1px var(--border-style) var(--color-border);
	}

	/* ── Empty State ── */
	.empty-state {
		flex: 1;
		display: flex; flex-direction: column; align-items: center; justify-content: center;
		gap: 8px;
	}
	.empty-glyph { font-size: 28px; color: var(--color-text-ghost); animation: blink 3s ease-in-out infinite; }
	.empty-text { font-family: var(--font-display); font-size: 12px; letter-spacing: 0.15em; color: var(--color-text-dim); }
	.empty-sub { font-family: var(--font-data); font-size: 11px; color: var(--color-text-ghost); }

	/* ── Project Block ── */
	.project-block { animation: boot-in 0.3s ease-out both; }
	.project-header {
		display: flex; align-items: center; justify-content: space-between;
		padding: 0 2px 6px;
	}
	.project-header-left { display: flex; align-items: center; gap: 8px; }
	.project-marker { font-size: 7px; color: var(--color-accent); }
	.project-name {
		font-family: var(--font-display);
		font-size: 11px; font-weight: 600; letter-spacing: 0.15em;
		color: var(--color-text-secondary);
	}
	.project-count {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-dim);
		border: 1px var(--border-style) var(--color-border);
		padding: 1px 6px; line-height: 1.3; border-radius: var(--radius);
	}
	.project-path { font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost); }

	/* ── Session List ── */
	.session-list { display: flex; flex-direction: column; gap: 3px; }

	/* ── Session Row ── */
	.session-row {
		display: flex; align-items: flex-start; gap: 10px;
		padding: 10px 14px;
		background: var(--color-surface);
		border: 1px var(--border-style) var(--color-border);
		border-left: var(--row-border-left-width) var(--border-style) var(--row-color, var(--color-text-ghost));
		border-radius: var(--radius);
		cursor: pointer; text-align: left; width: 100%;
		transition: all 0.12s ease;
		animation: boot-in 0.3s ease-out both;
		position: relative;
	}
	.session-row:hover {
		background: var(--color-surface-hover);
		border-color: var(--color-border-bright);
		border-left-color: var(--row-color);
	}
	.session-row:hover .session-slug { color: var(--color-text-primary); }

	.session-viewing {
		background: var(--color-surface-hover);
		border-color: var(--color-accent)40;
	}

	.session-waiting {
		background: var(--color-urgent-dim);
		border-color: var(--color-border-bright);
	}
	.session-waiting::after {
		content: '';
		position: absolute; inset: 0;
		pointer-events: none;
		border-radius: var(--radius);
		box-shadow: inset 0 0 16px var(--color-urgent-dim);
		animation: urgent-beacon 2s ease-in-out infinite;
	}

	/* ── Session Parts ── */
	.session-indicator {
		width: 18px;
		display: flex; align-items: center; justify-content: center;
		flex-shrink: 0; padding-top: 1px;
	}
	.indicator-glyph { font-size: 13px; line-height: 1; }

	.session-core {
		flex: 1; min-width: 0;
		display: flex; flex-direction: column; gap: 4px;
	}
	.session-id-row { display: flex; align-items: baseline; gap: 8px; }
	.session-slug {
		font-family: var(--font-body); font-size: 13px; font-weight: 500;
		color: var(--color-text-primary);
		transition: color 0.12s ease;
	}
	.session-activity-inline {
		font-family: var(--font-data); font-size: 11px; color: var(--color-text-dim);
	}

	.session-meta-row { display: flex; align-items: center; gap: 10px; font-size: 11px; }
	.session-model {
		font-family: var(--font-display); font-size: 9px; font-weight: 500;
		letter-spacing: 0.1em; color: var(--color-text-secondary); text-transform: uppercase;
	}
	.context-bar-wrap {
		width: 50px; height: 5px; background: var(--color-border);
		overflow: hidden; flex-shrink: 0; border-radius: 1px;
	}
	.context-bar-fill { height: 100%; transition: width 0.3s ease; border-radius: 1px; }
	.context-label { font-family: var(--font-data); font-size: 10px; font-variant-numeric: tabular-nums; }
	.session-branch { font-family: var(--font-data); font-size: 10px; color: var(--color-text-dim); }
	.session-source-small {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost); margin-left: auto;
	}

	.session-message {
		font-family: var(--font-body); font-size: 11px; color: var(--color-text-dim);
		overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
		margin: 0; line-height: 1.4;
	}

	.session-status-tag {
		font-family: var(--font-display); font-size: 9px; font-weight: 500;
		letter-spacing: 0.12em; padding: 3px 8px;
		border: 1px var(--border-style);
		border-radius: var(--radius); flex-shrink: 0; margin-top: 1px;
	}

	.session-elapsed {
		font-family: var(--font-data); font-size: 12px; color: var(--color-text-secondary);
		font-variant-numeric: tabular-nums; width: 64px; text-align: right;
		flex-shrink: 0; margin-top: 1px;
	}

	/* ── View Button ── */
	.session-focus-btn {
		opacity: 0;
		background: none;
		border: 1px var(--border-style) var(--color-border);
		color: var(--color-text-dim);
		width: 24px; height: 24px; font-size: 11px;
		display: flex; align-items: center; justify-content: center;
		cursor: pointer;
		border-radius: var(--radius);
		flex-shrink: 0; margin-top: 1px;
		transition: all 0.12s ease;
	}
	.session-row:hover .session-focus-btn { opacity: 1; }
	.session-focus-btn:hover {
		color: var(--color-accent);
		border-color: var(--color-accent);
		background: var(--color-surface-hover);
	}

	/* ── Conversation Panel ── */
	.conversation-panel {
		flex: 0 0 58%;
		display: flex;
		flex-direction: column;
		background: var(--color-bg);
		animation: slide-in 0.2s ease-out;
		overflow: hidden;
	}

	.conversation-header {
		display: flex; align-items: center; justify-content: space-between;
		padding: 10px 16px;
		border-bottom: 1px var(--border-style) var(--color-border);
		background: var(--color-surface);
		flex-shrink: 0;
	}
	.conversation-header-left { display: flex; align-items: center; gap: 10px; }
	.conversation-header-right { display: flex; align-items: center; gap: 6px; }
	.conversation-slug {
		font-family: var(--font-body); font-size: 13px; font-weight: 500;
		color: var(--color-text-primary);
	}
	.conversation-model {
		font-family: var(--font-display); font-size: 8px; font-weight: 500;
		letter-spacing: 0.1em; color: var(--color-text-dim); text-transform: uppercase;
	}
	.conversation-count {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost);
	}
	.conv-btn {
		background: none;
		border: 1px var(--border-style) var(--color-border);
		color: var(--color-text-secondary);
		width: 26px; height: 26px; font-size: 13px;
		display: flex; align-items: center; justify-content: center;
		cursor: pointer;
		border-radius: var(--radius);
		transition: all 0.12s ease;
	}
	.conv-btn:hover {
		border-color: var(--color-accent);
		color: var(--color-accent);
		background: var(--color-surface-hover);
	}

	.conversation-messages {
		flex: 1;
		overflow-y: auto;
		padding: 12px 16px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.conv-loading {
		flex: 1;
		display: flex; align-items: center; justify-content: center;
		font-family: var(--font-data); font-size: 11px; color: var(--color-text-ghost);
	}

	/* ── Message Types ── */
	.msg {
		font-family: var(--font-body);
		font-size: 12px;
		line-height: 1.55;
		word-wrap: break-word;
		border-radius: var(--radius);
	}

	.msg-user {
		padding: 8px 12px;
		background: var(--color-surface);
		border-left: 2px solid var(--color-accent);
		color: var(--color-text-primary);
	}
	.msg-time {
		font-family: var(--font-data); font-size: 9px; color: var(--color-text-ghost);
		display: block; margin-bottom: 4px;
	}

	.msg-assistant {
		padding: 6px 12px;
		color: var(--color-text-primary);
	}

	.msg-tool-use {
		padding: 6px 10px;
		background: var(--color-surface);
		border: 1px var(--border-style) var(--color-border);
		display: flex; align-items: center; gap: 8px;
		font-family: var(--font-data); font-size: 11px;
	}
	.tool-badge {
		font-family: var(--font-display); font-size: 8px; font-weight: 600;
		letter-spacing: 0.1em; text-transform: uppercase;
		color: var(--color-accent);
		padding: 1px 6px;
		border: 1px var(--border-style) var(--color-border);
		border-radius: var(--radius);
		flex-shrink: 0;
	}
	.tool-summary { color: var(--color-text-dim); }

	.msg-tool-result {
		padding: 6px 10px;
		background: var(--color-void);
		border: 1px var(--border-style) var(--color-border);
	}
	.msg-tool-result.error {
		border-color: var(--color-urgent)30;
	}
	.tool-error-label {
		font-family: var(--font-display); font-size: 8px; font-weight: 600;
		letter-spacing: 0.1em; color: var(--color-urgent);
		display: block; margin-bottom: 4px;
	}
	.tool-output {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-dim);
		margin: 0; white-space: pre-wrap; word-break: break-all;
		max-height: 120px; overflow-y: auto;
	}

	.msg-thinking {
		font-size: 11px; color: var(--color-text-ghost); font-style: italic;
		padding: 4px 12px;
	}
	.msg-thinking summary {
		cursor: pointer;
		font-family: var(--font-data); font-size: 10px;
		color: var(--color-text-ghost);
	}
	.msg-thinking p {
		margin: 4px 0 0; font-style: normal;
		font-family: var(--font-body); font-size: 11px; color: var(--color-text-dim);
	}

	.msg-system {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost);
		text-align: center; padding: 2px 12px;
	}

	/* ── Code rendering ── */
	:global(.code-block) {
		background: var(--color-void);
		padding: 8px 10px;
		border-radius: var(--radius);
		font-family: var(--font-data);
		font-size: 11px;
		overflow-x: auto;
		white-space: pre;
		margin: 4px 0;
		border: 1px var(--border-style) var(--color-border);
	}
	:global(.inline-code) {
		background: var(--color-surface);
		padding: 1px 4px;
		border-radius: 2px;
		font-family: var(--font-data);
		font-size: 0.9em;
	}

	@keyframes slide-in {
		from { transform: translateX(16px); opacity: 0; }
		to { transform: translateX(0); opacity: 1; }
	}

	/* ── Bottom Bar ── */
	.bottom-bar {
		display: flex; align-items: center; justify-content: center;
		gap: 20px; padding: 5px 20px;
		border-top: 1px var(--border-style) var(--color-border);
		background: var(--color-void);
		position: relative; z-index: 2;
		transition: background-color 0.3s ease;
	}
	.bottom-meta {
		font-family: var(--font-data); font-size: 9px; letter-spacing: 0.08em;
		color: var(--color-text-ghost);
	}
</style>
