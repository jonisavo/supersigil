import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLlmsTxt from 'starlight-llms-txt';
import mermaid from 'astro-mermaid';

export default defineConfig({
  base: '/supersigil',
  site: 'https://supersigil.dev',
  integrations: [
    mermaid({
      autoTheme: true,
    }),
    starlight({
      title: 'supersigil',
      description: 'Spec-driven development with AI agents.',
      expressiveCode: {
        shiki: {
          langAlias: {
            'supersigil-xml': 'xml',
          },
        },
      },
      plugins: [starlightLlmsTxt()],
      logo: {
        light: './src/assets/logo-light.svg',
        dark: './src/assets/logo-dark.svg',
        replacesTitle: false,
      },
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/supersigil/supersigil' },
      ],
      sidebar: [
        { label: 'Introduction', slug: 'introduction' },
        {
          label: 'Spec Explorer',
          link: '/explore/',
          attrs: { target: '_blank', class: 'external-link' },
        },
        {
          label: 'Quickstart',
          items: [
            { label: 'Quickstart', slug: 'getting-started' },
            { label: 'Existing Projects', slug: 'getting-started/existing-project' },
            { label: 'Deep Dive Tutorial', slug: 'getting-started/first-spec' },
          ],
        },
        {
          label: 'Workflows',
          items: [
            { label: 'Editor Setup', slug: 'guides/editor-setup' },
            { label: 'Working with AI Agents', slug: 'guides/ai-agents' },
            { label: 'Architecture Decisions', slug: 'guides/architecture-decisions' },
            { label: 'CI Verification', slug: 'guides/ci-verification' },
            { label: 'Executable Examples', slug: 'guides/executable-examples' },
            { label: 'Graph Explorer', slug: 'guides/graph-explorer' },
          ],
        },
        {
          label: 'Concepts',
          items: [
            { label: 'How It Works', slug: 'concepts' },
            { label: 'The Component Graph', slug: 'concepts/component-graph' },
            { label: 'Verification', slug: 'concepts/verification' },
            { label: 'Evidence Sources', slug: 'concepts/evidence-sources' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CLI Commands', slug: 'reference/cli' },
            { label: 'Components', slug: 'reference/components' },
            { label: 'Configuration', slug: 'reference/configuration' },
          ],
        },
      ],
      customCss: ['./src/styles/custom.css'],
      head: [
        {
          tag: 'link',
          attrs: {
            rel: 'preconnect',
            href: 'https://fonts.googleapis.com',
          },
        },
        {
          tag: 'link',
          attrs: {
            rel: 'preconnect',
            href: 'https://fonts.gstatic.com',
            crossorigin: true,
          },
        },
        {
          tag: 'link',
          attrs: {
            rel: 'stylesheet',
            href: 'https://fonts.googleapis.com/css2?family=Crimson+Pro:ital,wght@0,300;0,400;0,500;0,600;0,700;1,300;1,400&family=Outfit:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap',
          },
        },
      ],
      favicon: '/favicon.svg',
    }),
  ],
});
