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

	interface DiscoverableProject {
		path: string;
		display_name: string;
		agents: string[];
		active_sessions: number;
	}

	const THEMES = ['nightfall', 'fieldcom', 'warmdesk'] as const;
	type Theme = typeof THEMES[number];

	let groups: ProjectGroup[] = $state([]);
	let lastScan = $state('');
	let clock = $state('');
	let booted = $state(false);
	let scanning = $state(false);
	let theme: Theme = $state((localStorage.getItem('observatory-theme') as Theme) || 'nightfall');

	// Conversation panel state
	let viewingSession: Session | null = $state(null);
	let conversationData: ConversationData | null = $state(null);
	let conversationLoading = $state(false);

	// Projects view state
	type ViewMode = 'sessions' | 'projects';
	let viewMode: ViewMode = $state('sessions');
	let projects: DiscoverableProject[] = $state([]);
	let projectsLoaded = $state(false);
	let projectsLoading = $state(false);
	let launchingPath: string | null = $state(null);

	$effect(() => {
		document.documentElement.setAttribute('data-theme', theme);
		localStorage.setItem('observatory-theme', theme);
	});

	onMount(async () => {
		document.documentElement.setAttribute('data-theme', theme);
		setTimeout(() => { booted = true; }, 100);

		const updateClock = () => {
			clock = new Date().toLocaleTimeString('en-US', { hour12: false });
		};
		updateClock();
		const clockInterval = setInterval(updateClock, 1000);

		try {
			scanning = true;
			groups = await invoke('get_session_groups');
			lastScan = new Date().toLocaleTimeString('en-US', { hour12: false });
			setTimeout(() => { scanning = false; }, 600);
		} catch (e) {
			console.error('Failed to scan sessions:', e);
			scanning = false;
		}

		await listen<ProjectGroup[]>('sessions-changed', async (event) => {
			scanning = true;
			groups = event.payload;
			lastScan = new Date().toLocaleTimeString('en-US', { hour12: false });
			setTimeout(() => { scanning = false; }, 600);

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

	async function loadProjects() {
		if (projectsLoading) return;
		projectsLoading = true;
		try {
			projects = await invoke('get_discoverable_projects');
			projectsLoaded = true;
		} catch (e) {
			console.error('Failed to load projects:', e);
		}
		projectsLoading = false;
	}

	async function switchView(mode: ViewMode) {
		viewMode = mode;
		if (mode === 'projects' && !projectsLoaded) {
			await loadProjects();
		}
		if (mode === 'projects') {
			closeConversation();
		}
	}

	async function refreshProjects() {
		projectsLoaded = false;
		await loadProjects();
	}

	async function launchSession(project: DiscoverableProject, agent?: string) {
		launchingPath = project.path;
		try {
			await invoke('launch_claude_session', {
				path: project.path,
				agent: agent || null
			});
		} catch (e) {
			console.error('Failed to launch session:', e);
		}
		setTimeout(() => { launchingPath = null; }, 1500);
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

	interface ProcessedMessage {
		type: 'user' | 'assistant' | 'tool_op' | 'thinking' | 'system';
		text?: string;
		timestamp?: string | null;
		toolName?: string;
		toolSummary?: string;
		toolResult?: string | null;
		isError?: boolean;
	}

	function processMessages(msgs: ConversationMessage[]): ProcessedMessage[] {
		const out: ProcessedMessage[] = [];
		let i = 0;
		while (i < msgs.length) {
			const msg = msgs[i];

			if (msg.messageType === 'tool_use') {
				// Look ahead for matching tool_result
				const next = msgs[i + 1];
				const result = next?.messageType === 'tool_result' ? next : null;
				out.push({
					type: 'tool_op',
					toolName: msg.toolName || 'unknown',
					toolSummary: shortenToolSummary(msg.toolInputSummary || ''),
					toolResult: result?.toolResultContent,
					isError: result?.isError ?? false,
				});
				if (result) i += 2; else i += 1;
				continue;
			}

			if (msg.messageType === 'tool_result') {
				// Orphan tool_result (no preceding tool_use) — show if error
				if (msg.isError) {
					out.push({
						type: 'tool_op',
						toolName: '?',
						toolSummary: '',
						toolResult: msg.toolResultContent,
						isError: true,
					});
				}
				i++; continue;
			}

			if (msg.messageType === 'text' && msg.role === 'user') {
				out.push({ type: 'user', text: msg.text || '', timestamp: msg.timestamp });
			} else if (msg.messageType === 'text' && msg.role === 'assistant') {
				out.push({ type: 'assistant', text: msg.text || '' });
			} else if (msg.messageType === 'thinking') {
				out.push({ type: 'thinking', text: msg.text || '' });
			} else if (msg.role === 'system') {
				out.push({ type: 'system', text: msg.text || '' });
			}
			i++;
		}
		return out;
	}

	function shortenToolSummary(summary: string): string {
		// "Edit: ~/SomeProject/SubDir/deep/path/file.tsx" → "Edit: .../path/file.tsx"
		return summary.replace(/\/Users\/[^/]+\//g, '~/').replace(/(~\/[^/]+\/[^/]+\/)(.+\/)/g, (_, prefix, middle) => {
			const parts = middle.split('/').filter(Boolean);
			if (parts.length > 2) return prefix + '.../' + parts.slice(-1)[0] + '/';
			return prefix + middle;
		});
	}

	function isSuccessResult(result: string | null | undefined): boolean {
		if (!result) return true; // no result content = assume success
		return result.includes('has been updated successfully') ||
			result.includes('successfully') ||
			result.trim() === '' ||
			result === '[Output saved to file]';
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
	<div class="scan-beam" class:active={scanning}></div>

	<header class="top-bar">
		<div class="top-bar-left">
			<span class="observatory-title">OBSERVATORY</span>
			<span class="top-bar-divider">│</span>
			<div class="view-toggle">
				<button
					class="view-toggle-btn"
					class:active={viewMode === 'sessions'}
					onclick={() => switchView('sessions')}
				>SESSIONS</button>
				<button
					class="view-toggle-btn"
					class:active={viewMode === 'projects'}
					onclick={() => switchView('projects')}
				>PROJECTS</button>
			</div>
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

	<main class="main-area" class:panel-open={viewingSession && viewMode === 'sessions'}>
		{#if viewMode === 'sessions'}
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
		{:else}
		<!-- Projects View -->
		<div class="projects-pane">
			<div class="projects-toolbar">
				<span class="projects-count">
					{#if projectsLoaded}
						{projects.length} projects
					{:else}
						—
					{/if}
				</span>
				<button class="conv-btn" onclick={refreshProjects} title="Refresh projects">↻</button>
			</div>

			{#if projectsLoading}
				<div class="empty-state">
					<div class="empty-glyph">◇</div>
					<p class="empty-text">SCANNING PROJECTS...</p>
				</div>
			{:else if projects.length === 0}
				<div class="empty-state">
					<div class="empty-glyph">◇</div>
					<p class="empty-text">NO PROJECTS FOUND</p>
					<p class="empty-sub">~/.claude/projects/</p>
				</div>
			{:else}
				<div class="projects-list">
					{#each projects as project, pi}
						<div
							class="project-card"
							class:project-active={project.active_sessions > 0}
							class:project-launching={launchingPath === project.path}
							style="animation-delay: {pi * 40}ms;"
						>
							<div class="project-card-header">
								<div class="project-card-left">
									<span class="project-card-marker" class:marker-active={project.active_sessions > 0}>
										{project.active_sessions > 0 ? '■' : '□'}
									</span>
									<span class="project-card-name">{project.display_name}</span>
									{#if project.active_sessions > 0}
										<span class="project-card-sessions">{project.active_sessions} active</span>
									{/if}
								</div>
								<div class="project-card-actions">
									{#if project.agents.length > 0}
										<div class="agent-dropdown-wrap">
											<select
												class="agent-select"
												onchange={(e) => {
													const target = e.target as HTMLSelectElement;
													const val = target.value;
													if (val) {
														launchSession(project, val);
														target.value = '';
													}
												}}
											>
												<option value="">agents</option>
												{#each project.agents as agent}
													<option value={agent}>{agent}</option>
												{/each}
											</select>
										</div>
									{/if}
									<button
										class="launch-btn"
										onclick={() => launchSession(project)}
										title="Launch new Claude session"
									>+</button>
								</div>
							</div>
							<span class="project-card-path">{shortPath(project.path)}</span>
							{#if project.agents.length > 0}
								<div class="project-card-agents">
									{#each project.agents as agent}
										<span class="agent-tag">{agent}</span>
									{/each}
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</div>
		{/if}

		{#if viewingSession && viewMode === 'sessions'}
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
						{@const processed = processMessages(conversationData.messages)}
						{#each processed as pm, pi}
							{#if pm.type === 'user'}
								<div class="msg msg-user">
									<span class="msg-time">{formatTime(pm.timestamp ?? null)}</span>
									<div class="msg-content">{@html renderText(pm.text || '')}</div>
								</div>
							{:else if pm.type === 'assistant'}
								<div class="msg msg-assistant">
									<div class="msg-content">{@html renderText(pm.text || '')}</div>
								</div>
							{:else if pm.type === 'tool_op'}
								{@const success = !pm.isError && isSuccessResult(pm.toolResult)}
								{@const prevIsTool = pi > 0 && processed[pi - 1].type === 'tool_op'}
								<div class="msg msg-tool-op" class:tool-group-item={prevIsTool} class:tool-error={pm.isError}>
									<span class="tool-name">{pm.toolName}</span>
									<span class="tool-target">{pm.toolSummary}</span>
									{#if success}
										<span class="tool-ok">✓</span>
									{:else if pm.isError}
										<details class="tool-err-details">
											<summary class="tool-err-marker">✗</summary>
											<pre class="tool-err-content">{pm.toolResult || ''}</pre>
										</details>
									{:else if pm.toolResult}
										<details class="tool-output-details">
											<summary class="tool-out-marker">⋯</summary>
											<pre class="tool-out-content">{pm.toolResult}</pre>
										</details>
									{/if}
								</div>
							{:else if pm.type === 'thinking'}
								<details class="msg msg-thinking">
									<summary>thinking</summary>
									<p>{pm.text || ''}</p>
								</details>
							{:else if pm.type === 'system'}
								<div class="msg msg-system">{pm.text || ''}</div>
							{/if}
						{/each}
					{/if}
				</div>
			</div>
		{/if}
	</main>

	<footer class="bottom-bar">
		{#if viewMode === 'sessions'}
			<span class="bottom-meta">CLICK TO VIEW</span>
			<span class="bottom-meta">→ GO TO TERMINAL</span>
			<span class="bottom-meta">POLL 10s</span>
		{:else}
			<span class="bottom-meta">+ LAUNCH SESSION</span>
			<span class="bottom-meta">AGENTS VIA DROPDOWN</span>
			<span class="bottom-meta">↻ REFRESH</span>
		{/if}
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
		opacity: 0;
		z-index: 10;
		pointer-events: none;
	}
	.scan-beam.active {
		opacity: 0.15;
		animation: scan 0.5s ease-out forwards;
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
		border: none;
		color: var(--color-text-dim);
		font-size: 14px;
		width: 28px; height: 28px;
		display: flex; align-items: center; justify-content: center;
		cursor: pointer;
		border-radius: var(--radius);
		transition: all 0.15s ease;
	}
	.theme-btn:hover {
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
	}
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
		padding: 16px 20px;
		display: flex;
		flex-direction: column;
		gap: 22px;
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
		padding: 0 4px 8px;
	}
	.project-header-left { display: flex; align-items: center; gap: 8px; }
	.project-marker { font-size: 7px; color: var(--color-accent); }
	.project-name {
		font-family: var(--font-display);
		font-size: 11px; font-weight: 600; letter-spacing: 0.15em;
		color: var(--color-text-secondary);
	}
	.project-count {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost);
	}
	.project-path { font-family: var(--font-data); font-size: 10px; color: var(--color-text-ghost); }

	/* ── Session List ── */
	.session-list { display: flex; flex-direction: column; gap: 6px; }

	/* ── Session Row ── */
	.session-row {
		display: flex; align-items: flex-start; gap: 10px;
		padding: 10px 14px;
		background: transparent;
		border: none;
		border-left: var(--row-border-left-width) solid var(--row-color, var(--color-text-ghost));
		border-bottom: 1px solid var(--color-border);
		border-radius: var(--radius);
		cursor: pointer; text-align: left; width: 100%;
		transition: all 0.12s ease;
		animation: boot-in 0.3s ease-out both;
		position: relative;
	}
	.session-row:hover {
		background: var(--color-surface);
	}
	.session-row:hover .session-slug { color: var(--color-text-primary); }

	.session-viewing {
		background: var(--color-surface);
	}

	.session-waiting {
		background: var(--color-urgent-dim);
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
		border-radius: var(--radius); flex-shrink: 0; margin-top: 1px;
	}

	.session-elapsed {
		font-family: var(--font-data); font-size: 12px; color: var(--color-text-secondary);
		font-variant-numeric: tabular-nums; width: 64px; text-align: right;
		flex-shrink: 0; margin-top: 1px;
	}

	/* ── Focus Button ── */
	.session-focus-btn {
		opacity: 0;
		background: none;
		border: none;
		color: var(--color-text-ghost);
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
		border: none;
		color: var(--color-text-dim);
		width: 26px; height: 26px; font-size: 13px;
		display: flex; align-items: center; justify-content: center;
		cursor: pointer;
		border-radius: var(--radius);
		transition: all 0.12s ease;
	}
	.conv-btn:hover {
		color: var(--color-accent);
		background: var(--color-surface-hover);
	}

	.conversation-messages {
		flex: 1;
		overflow-y: auto;
		padding: 14px 18px;
		display: flex;
		flex-direction: column;
		gap: 4px;
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
		padding: 10px 12px;
		background: var(--color-surface);
		border-left: 2px solid var(--color-accent);
		color: var(--color-text-primary);
		margin-top: 6px;
	}
	.msg-time {
		font-family: var(--font-data); font-size: 9px; color: var(--color-text-ghost);
		display: block; margin-bottom: 4px;
	}

	.msg-assistant {
		padding: 6px 12px;
		color: var(--color-text-primary);
	}

	/* ── Tool Operations (merged tool_use + tool_result) ── */
	.msg-tool-op {
		display: flex; align-items: center; gap: 6px;
		padding: 3px 10px;
		font-family: var(--font-data); font-size: 10px;
		color: var(--color-text-dim);
		margin-top: 2px;
	}
	.msg-tool-op.tool-group-item {
		margin-top: 0;
	}
	.tool-name {
		font-family: var(--font-display); font-size: 8px; font-weight: 600;
		letter-spacing: 0.08em; text-transform: uppercase;
		color: var(--color-text-ghost);
		flex-shrink: 0;
		min-width: 32px;
	}
	.tool-target {
		color: var(--color-text-dim);
		overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
		flex: 1; min-width: 0;
	}
	.tool-ok {
		color: var(--color-status-active);
		font-size: 10px; flex-shrink: 0;
	}
	.tool-err-details, .tool-output-details {
		flex-shrink: 0;
	}
	.tool-err-marker {
		color: var(--color-urgent); cursor: pointer; font-size: 10px;
	}
	.tool-out-marker {
		color: var(--color-text-ghost); cursor: pointer; font-size: 10px;
	}
	.tool-err-content, .tool-out-content {
		font-family: var(--font-data); font-size: 10px; color: var(--color-text-dim);
		margin: 4px 0 0; white-space: pre-wrap; word-break: break-all;
		max-height: 120px; overflow-y: auto;
		padding: 4px 8px;
		background: var(--color-void);
		border-radius: var(--radius);
	}
	.msg-tool-op.tool-error {
		border-left: 2px solid var(--color-urgent);
	}

	.msg-thinking {
		font-size: 10px; color: var(--color-text-ghost);
		padding: 2px 12px;
	}
	.msg-thinking summary {
		cursor: pointer;
		font-family: var(--font-data); font-size: 9px;
		color: var(--color-text-ghost);
		letter-spacing: 0.05em;
	}
	.msg-thinking p {
		margin: 4px 0 0;
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
		background: var(--color-void);
		position: relative; z-index: 2;
		transition: background-color 0.3s ease;
	}
	.bottom-meta {
		font-family: var(--font-data); font-size: 9px; letter-spacing: 0.08em;
		color: var(--color-text-ghost);
	}

	/* ── View Toggle ── */
	.view-toggle {
		display: flex;
		gap: 0;
		border: 1px var(--border-style) var(--color-border);
		border-radius: var(--radius);
		overflow: hidden;
	}
	.view-toggle-btn {
		background: none;
		border: none;
		font-family: var(--font-display);
		font-size: 9px;
		font-weight: 500;
		letter-spacing: 0.12em;
		color: var(--color-text-ghost);
		padding: 4px 12px;
		cursor: pointer;
		transition: all 0.15s ease;
	}
	.view-toggle-btn:hover {
		color: var(--color-text-dim);
		background: var(--color-surface-hover);
	}
	.view-toggle-btn.active {
		color: var(--color-accent);
		background: var(--color-surface);
	}

	/* ── Projects Pane ── */
	.projects-pane {
		flex: 1;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}
	.projects-toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 20px;
		border-bottom: 1px var(--border-style) var(--color-border);
		flex-shrink: 0;
	}
	.projects-count {
		font-family: var(--font-data);
		font-size: 11px;
		color: var(--color-text-dim);
	}
	.projects-list {
		flex: 1;
		overflow-y: auto;
		padding: 16px 20px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	/* ── Project Card ── */
	.project-card {
		padding: 12px 14px;
		border: 1px var(--border-style) var(--color-border);
		border-radius: var(--radius);
		display: flex;
		flex-direction: column;
		gap: 6px;
		transition: all 0.12s ease;
		animation: boot-in 0.3s ease-out both;
	}
	.project-card:hover {
		background: var(--color-surface);
		border-color: var(--color-border-bright);
	}
	.project-active {
		border-left: 3px solid var(--color-status-active);
	}
	.project-launching {
		opacity: 0.6;
		pointer-events: none;
	}
	.project-card-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
	}
	.project-card-left {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.project-card-marker {
		font-size: 8px;
		color: var(--color-text-ghost);
		flex-shrink: 0;
	}
	.project-card-marker.marker-active {
		color: var(--color-status-active);
	}
	.project-card-name {
		font-family: var(--font-body);
		font-size: 13px;
		font-weight: 500;
		color: var(--color-text-primary);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.project-card-sessions {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-status-active);
		flex-shrink: 0;
	}
	.project-card-actions {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-shrink: 0;
	}
	.project-card-path {
		font-family: var(--font-data);
		font-size: 10px;
		color: var(--color-text-ghost);
	}
	.project-card-agents {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.agent-tag {
		font-family: var(--font-data);
		font-size: 9px;
		color: var(--color-text-dim);
		background: var(--color-surface);
		padding: 2px 6px;
		border-radius: var(--radius);
		border: 1px solid var(--color-border);
	}

	/* ── Agent Dropdown ── */
	.agent-dropdown-wrap {
		position: relative;
	}
	.agent-select {
		appearance: none;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		color: var(--color-text-dim);
		font-family: var(--font-data);
		font-size: 10px;
		padding: 3px 20px 3px 8px;
		border-radius: var(--radius);
		cursor: pointer;
		transition: all 0.12s ease;
		min-width: 70px;
	}
	.agent-select:hover {
		border-color: var(--color-border-bright);
		color: var(--color-text-secondary);
	}
	.agent-select:focus {
		outline: none;
		border-color: var(--color-accent);
	}

	/* ── Launch Button ── */
	.launch-btn {
		width: 26px;
		height: 26px;
		display: flex;
		align-items: center;
		justify-content: center;
		background: none;
		border: 1px solid var(--color-border);
		color: var(--color-text-dim);
		font-size: 16px;
		font-weight: 300;
		border-radius: var(--radius);
		cursor: pointer;
		transition: all 0.15s ease;
		line-height: 1;
	}
	.launch-btn:hover {
		border-color: var(--color-accent);
		color: var(--color-accent);
		background: var(--color-surface);
	}
</style>
