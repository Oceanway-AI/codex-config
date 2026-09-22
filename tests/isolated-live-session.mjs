// Private JSONL control channel for the optional live native-app harness.
// Credentials enter over stdin, never as command-line arguments or report fields.
import readline from 'node:readline';
import * as harness from './isolated-live-harness.mjs';

let context;
let secret = '';
const tests = new Map();
const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  let command;
  try {
    command = JSON.parse(line);
    let result;
    switch (command.action) {
      case 'start':
        secret = command.options.apiKey;
        context = await harness.startNative(command.options);
        result = { root: context.root, pid: context.child.pid, capabilities: context.capabilities };
        break;
      case 'generate':
        result = await harness.startImageTest(context, command.request);
        tests.set(result.id, result);
        break;
      case 'status':
        result = await harness.inspectImageTest(context, tests.get(command.jobId));
        break;
      case 'screenshot':
        await context.page.screenshot({ path: command.path });
        result = { path: command.path };
        break;
      case 'stop':
        await harness.stopNative(context);
        result = { stopped: true };
        break;
      default:
        throw new Error('Unsupported private test action.');
    }
    console.log(JSON.stringify({ id: command.id, result }));
    if (command.action === 'stop') break;
  } catch (error) {
    const message = String(error.message || error);
    console.log(JSON.stringify({
      id: command?.id,
      error: secret ? message.replaceAll(secret, '[REDACTED]') : message,
    }));
  }
}
