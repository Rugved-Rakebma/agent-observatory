<script lang="ts">
	import '../app.css';
	let { children } = $props();
</script>

<svelte:head>
	<title>OBSERVATORY</title>
	<link rel="preconnect" href="https://fonts.googleapis.com" />
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
	<link href="https://fonts.googleapis.com/css2?family=Share+Tech+Mono&family=Orbitron:wght@400;500;600;700&family=IBM+Plex+Mono:wght@400;500;600&family=Bitter:wght@400;500;600;700&family=Source+Code+Pro:wght@400;500;600&family=Chakra+Petch:wght@400;500;600;700&family=DM+Mono:wght@400;500&display=swap" rel="stylesheet" />
</svelte:head>

{@render children()}
