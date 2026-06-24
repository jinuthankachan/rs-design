#!/usr/bin/env node
//
// mock-byok-openai.mjs — a loopback OpenAI-compatible streaming provider for the
// CP7 e2e (BYOK round-trip without a live API key).
//
// The daemon's `/api/proxy/openai/stream` route POSTs to
// `<baseUrl>/v1/chat/completions` (it auto-injects `/v1` when absent) with
// `{ model, messages, stream: true }` and `Authorization: Bearer <apiKey>`, then
// parses the upstream SSE: `data: {choices:[{delta:{content}}]}` chunks read by
// `extractOpenAIText`, terminated by `data: [DONE]`. The daemon re-emits these as
// its own `start` / `delta` / `end` SSE events to the webview.
//
// The SSRF guard intentionally allows loopback (so local Ollama works), so a
// 127.0.0.1 mock is a faithful, offline-safe BYOK upstream. This server answers
// ANY path so path normalization (`/v1` injection) never matters, and streams a
// short two-token completion so the e2e can assert real provider content
// ("mock-token") arrives through the seam.
//
// Prints `MOCK_URL=http://127.0.0.1:<port>` on stdout once listening, then serves
// until killed. Env: MOCK_TOKEN (the delta text to emit, default "mock-token").
import http from 'node:http';

const TOKEN = process.env.MOCK_TOKEN || 'mock-token';

const server = http.createServer((req, res) => {
  // Drain the request body (the daemon sends a JSON payload) before replying.
  req.on('data', () => {});
  req.on('end', () => {
    res.writeHead(200, {
      'content-type': 'text/event-stream',
      'cache-control': 'no-cache',
      connection: 'keep-alive',
    });
    const chunk = (content) =>
      `data: ${JSON.stringify({ choices: [{ index: 0, delta: { content } }] })}\n\n`;
    // Two deltas + [DONE], the minimal shape extractOpenAIText/streamUpstreamSse expect.
    res.write(chunk(TOKEN + ' '));
    res.write(chunk('streamed'));
    res.write('data: [DONE]\n\n');
    res.end();
  });
});

server.listen(0, '127.0.0.1', () => {
  const { port } = server.address();
  process.stdout.write(`MOCK_URL=http://127.0.0.1:${port}\n`);
});

for (const sig of ['SIGINT', 'SIGTERM']) {
  process.on(sig, () => server.close(() => process.exit(0)));
}
