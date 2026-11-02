import test from 'node:test';
import assert from 'node:assert/strict';
import {visibleApps,backgroundApps,appTotal,monday,addDays,rangeFor,duration,percentage,escapeHtml,emptyDashboard,initials} from '../src/ui-model.mjs';

test('weeks begin on Monday, including Sunday and year boundaries',()=>{
  assert.equal(monday('2026-11-01'),'2026-10-26');
  assert.equal(monday('2026-11-02'),'2026-11-02');
  assert.equal(monday('2021-01-01'),'2020-12-28');
});
test('calendar ranges include the requested number of days across month boundaries',()=>{
  assert.deepEqual(rangeFor('7','2026-11-01'),{from:'2026-10-26',to:'2026-11-01'});
  assert.deepEqual(rangeFor('30','2026-11-01'),{from:'2026-10-03',to:'2026-11-01'});
  assert.deepEqual(rangeFor('month','2026-11-01'),{from:'2026-11-01',to:'2026-11-01'});
  assert.equal(addDays('2024-02-28',1),'2024-02-29');
  assert.equal(addDays('2026-12-31',1),'2027-01-01');
});
test('duration formatting keeps small actual samples and zero truthful',()=>{
  assert.equal(duration(3661),'1 ч 1 мин');
  assert.equal(duration(43),'43 сек');
  assert.equal(duration(0),'0 сек');
  assert.equal(duration(0,true),'0 мин');
  assert.equal(percentage(100,0),0);
  assert.equal(percentage(20,10),100);
});
test('untrusted application metadata is escaped before HTML rendering',()=>{
  assert.equal(escapeHtml('<img src="x" onerror=\'alert(1)\'>'),'&lt;img src=&quot;x&quot; onerror=&#39;alert(1)&#39;&gt;');
  assert.equal(initials('Visual Studio Code'),'VS');
});
test('disconnected and first-use state contains no invented activity',()=>{
  const value=emptyDashboard('2026-11-01','2026-11-01');
  assert.equal(value.total_seconds,0);assert.equal(value.idle_seconds,0);
  assert.equal(value.change_percent,null);assert.deepEqual(value.apps,[]);assert.deepEqual(value.hours,[]);
});

test('parallel application percentages use exposure total, not the computer clock',()=>{
  const d={total_seconds:3600,app_total_seconds:7200,apps:[{seconds:3600},{seconds:3600}]};
  assert.equal(appTotal(d),7200);assert.equal(percentage(d.apps[0].seconds,appTotal(d)),50);
  assert.equal(appTotal({...d,app_total_seconds:undefined}),7200);
  assert.equal(appTotal(emptyDashboard('2026-11-01','2026-11-01')),0);
});

test('visible apps lead lists while background-only apps remain available',()=>{
 const apps=[{id:'a',seconds:0,running_seconds:100,background_seconds:100},{id:'b',seconds:30,running_seconds:30,background_seconds:0}];
 assert.deepEqual(visibleApps(apps).map(a=>a.id),['b']);assert.deepEqual(backgroundApps(apps).map(a=>a.id),['b','a']);
 assert.equal(emptyDashboard('2026-11-02','2026-11-02').spotify_seconds,0);
});
