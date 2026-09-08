import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runAutoConfiguration } from '../src/auto-configure.js';
const ready = { configured:true,hasApiKey:true,imagegenCliConfigured:true };
test('resume checks saved configuration before restart without rewriting', async () => {
 for (const resumeFrom of ['checking', 'restarting']) {
  const calls=[];
  await runAutoConfiguration({resumeFrom, values:{},invoke:async name=>{
   calls.push(name);return name==='get_config_status'?ready:{restarted:true};
  },onStage:()=>{},onConfigured:()=>{}});
  assert.deepEqual(calls,['get_config_status','restart_codex']);
 }
});
test('resume cannot bypass a failed configuration check', async () => {
 const calls=[];
 await assert.rejects(runAutoConfiguration({resumeFrom:'restarting',values:{},invoke:async name=>{
  calls.push(name);return {};
 },onStage:()=>{},onConfigured:()=>{}}));
 assert.deepEqual(calls,['get_config_status']);
});
test('one action writes, checks and restarts in order without confirmation', async () => {
 const calls=[], stages=[];
 await runAutoConfiguration({values:{apiKey:'fake'},invoke:async name=>{
  calls.push(name);return name==='get_config_status'?ready:{restarted:true};
 },onStage:phase=>stages.push(phase),onConfigured:()=>{}});
 assert.deepEqual(calls,['configure_provider','get_config_status','restart_codex']);
 assert.deepEqual(stages,['writing','checking','restarting','complete']);
});
test('write or readback failure stops restart and never announces completion', async () => {
 for (const failWrite of [false,true]) {
  const calls=[], stages=[];
  await assert.rejects(runAutoConfiguration({values:{},invoke:async name=>{
   calls.push(name);if(failWrite)throw new Error('write failed');return {};
  },onStage:phase=>stages.push(phase),onConfigured:()=>{}}));
  assert.ok(!calls.includes('restart_codex'));assert.ok(!stages.includes('complete'));
 }
});
test('restart failure is surfaced without retry or fake success',async()=>{
 const stages=[],calls=[];
 await assert.rejects(runAutoConfiguration({values:{},invoke:async name=>{
  calls.push(name);return name==='get_config_status'?ready:{restarted:false,message:'quit refused'};
 },onStage:phase=>stages.push(phase),onConfigured:()=>{}}),/quit refused/);
 assert.equal(calls.filter(x=>x==='restart_codex').length,1);
 assert.ok(!stages.includes('complete'));
});
