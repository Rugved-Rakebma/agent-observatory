import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],

	// Tauri expects a fixed port in dev mode
	server: {
		port: 1420,
		strictPort: true,
	},

	// Tauri uses env vars for host detection
	envPrefix: ['VITE_', 'TAURI_'],
});
