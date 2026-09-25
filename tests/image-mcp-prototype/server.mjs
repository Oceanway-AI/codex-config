import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { isolatedPaths, providerReader, ImageService, toolResult } from './engine.mjs';

const paths = await isolatedPaths();
const service = new ImageService({ ...paths, readProvider: providerReader(paths.home) });
const server = new McpServer({ name: 'oceanway-images', version: '0.0.0-acceptance' });
server.registerTool('generate_images', {
  title: 'Generate or edit images with the current image provider',
  description: 'Create actual raster images when the user naturally asks for pictures, posters, visual assets, variations, or image edits. Uses the current configured OceanWay provider and API key automatically. Default image model is gpt-image-2; honor an explicitly requested model. For reference-based edits pass the ORIGINAL attachment or previous-output file paths, never text substitutes. Count is the user-requested total, not a per-request limit. For different styles or subjects provide one prompt per slot. Do not call for analysis-only or prompt-writing requests. This tool sends paid image requests. Poll get_image_job until terminal, and display the saved image paths. No CLI or imagegen skill is needed.',
  inputSchema: {
    prompt: z.string().optional().describe('Shared image description when all requested versions share one subject/style.'),
    prompts: z.array(z.string().min(1)).optional().describe('One prompt per output when styles or subjects differ; length must equal count.'),
    count: z.number().int().positive().default(1).describe('The exact total requested by the user.'),
    model: z.string().min(1).default('gpt-image-2'),
    size: z.string().min(1).default('1024x1024'),
    reference_paths: z.array(z.string()).default([]).describe('Ordered absolute paths of original reference images.'),
    workspace_directory: z.string().min(1).describe('Absolute current task working directory; results are saved under output/images.'),
  },
  annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
}, async args => toolResult(await service.start(args)));

server.registerTool('get_image_job', {
  description: 'Wait for and inspect a previously submitted image job without submitting or billing any new request. Poll while running. Returns counts, original file paths, previews, request IDs and failures.',
  inputSchema: { job_id: z.string(), wait_seconds: z.number().min(0).max(30).default(20) },
  annotations: { readOnlyHint: true, openWorldHint: false },
}, async ({ job_id, wait_seconds }) => toolResult(await service.wait(job_id, wait_seconds)));

server.registerTool('cancel_image_job', {
  description: 'Stop queued image requests only when the user requests cancellation. In-flight requests may still complete or incur charges. Already generated images are preserved.',
  inputSchema: { job_id: z.string() },
  annotations: { readOnlyHint: false, destructiveHint: false, openWorldHint: false },
}, async ({ job_id }) => toolResult(await service.cancel(job_id)));

await service.audit({ event: 'server-start', pid: process.pid, codexHome: paths.home });
await server.connect(new StdioServerTransport());
process.stdin.on('end', () => {
  for (const job of service.jobs.values()) service.cancel(job.id).catch(() => {});
});
