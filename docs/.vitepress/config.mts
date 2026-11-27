import { defineConfig } from 'vitepress'

const flag = (name: string) => process.argv.some((a) => a === name || a.startsWith(name + '='))

export default defineConfig({
  // GitHub Pages serves the site under /scriber/ (docs.yml sets DOCS_BASE); local dev stays at /.
  base: process.env.DOCS_BASE ?? '/',
  title: 'Scriber',
  description: '2D CAD for laser cutters, made for AI agents and the people who steer them.',
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ['README.md'],
  vite: {
    // Dev server defaults. Vite lets this config win over CLI flags, so a flag given on the command
    // line (`npm run docs:dev -- --host 0.0.0.0 --port 5191`, as lerd does) leaves its default out.
    server: {
      host: flag('--host') ? undefined : '127.0.0.1',
      port: flag('--port') ? undefined : 5191,
      strictPort: true,
      allowedHosts: ['.test', '.localhost'],
    },
  },
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/ai-first' },
      { text: 'Libraries', link: '/library/make-a-material-library' },
      { text: 'AI agents', link: '/agents/working-with-agents' },
      { text: 'Reference', link: '/reference/file-formats' },
    ],
    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Why AI-first', link: '/guide/ai-first' },
          { text: 'Connecting agents', link: '/agents/connecting' },
          { text: 'Installation', link: '/guide/installation' },
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'The screen', link: '/guide/the-screen' },
          { text: 'Themes', link: '/guide/themes' },
          { text: 'Drawing', link: '/guide/drawing' },
          { text: 'Editing', link: '/guide/editing' },
          { text: 'Constraints and dimensions', link: '/guide/constraints' },
          { text: 'Layers and laser operations', link: '/guide/layers' },
          { text: 'Machines and materials', link: '/guide/machines-and-materials' },
          { text: 'Importing from xTool Studio', link: '/importing-xtool-materials' },
          { text: 'Exporting', link: '/guide/exporting' },
          { text: 'Preview and check', link: '/guide/preview-and-check' },
          { text: 'Several documents', link: '/guide/documents' },
          { text: 'Copy and paste', link: '/guide/copy-and-paste' },
          { text: 'Review comments', link: '/guide/comments' },
        ],
      },
      {
        text: 'Material and machine libraries',
        items: [
          { text: 'Make your own material library', link: '/library/make-a-material-library' },
          { text: 'Share a library', link: '/library/share-a-library' },
          { text: 'Library repositories', link: '/library-repositories' },
        ],
      },
      {
        text: 'AI agents',
        items: [
          { text: 'How agents work with Scriber', link: '/agents/working-with-agents' },
          { text: 'MCP tool reference', link: '/agents/mcp-tools' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'File formats', link: '/reference/file-formats' },
          { text: 'xTool Studio .xs format', link: '/xs-format' },
        ],
      },
    ],
    search: { provider: 'local' },
    editLink: {
      pattern: 'https://github.com/ExeQue/scriber/edit/main/docs/:path',
      text: 'Edit this page on GitHub',
    },
    socialLinks: [{ icon: 'github', link: 'https://github.com/ExeQue/scriber' }],
  },
})
