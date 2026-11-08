import test from 'node:test';
import assert from 'node:assert/strict';
import {navigationPayload} from '../browser-extension/browser-model.mjs';
import {popularSites,genreCoverage,extraSlides} from '../src/insights-model.mjs';
test('browser history drops URL secrets and private windows before transport',()=>{
 const m={id:'visit-1',url:'https://www.youtube.com/watch?token=secret#private',title:'Video title',active:true};
 assert.deepEqual(navigationPayload(m,{active:true}),{id:'visit-1',url:'https://www.youtube.com',title:'',active:true});
 assert.equal(navigationPayload(m,{incognito:true}),null);
 assert.equal(navigationPayload({...m,url:'https://user:secret@example.org/private'},{details:true}),null);
 assert.equal(navigationPayload(m,{details:true,active:true}).url,'https://www.youtube.com/watch');
 assert.equal(navigationPayload(m,{active:false}).active,false);
});
test('history can reopen a YouTube video without keeping tokens or tracking parameters',()=>{
 const m={id:'video-1',url:'https://www.youtube.com/watch?v=abcDEF_1234&token=secret&si=private#fragment',title:'Video',active:true,navigation:true};
 const payload=navigationPayload(m,{active:true,details:true});assert.equal(payload.url,'https://www.youtube.com/watch?v=abcDEF_1234');assert.equal(payload.navigation,true);
 assert.equal(navigationPayload({...m,url:'https://youtube.com.evil.test/watch?v=abcDEF_1234'},{details:true}).url,'https://youtube.com.evil.test/watch');
 assert.equal(navigationPayload(m,{details:false}).url,'https://www.youtube.com');
});
test('popular site histories combine subdomains without mixing adult sites',()=>{
 const rows=[{site:'YouTube',domain:'youtube.com',seconds:30,visits:1},{site:'YouTube',domain:'m.youtube.com',seconds:20,visits:2},{site:'Pornhub',adult:true,seconds:10,visits:1}];
 const all=popularSites(rows);assert.equal(all[0].seconds,50);assert.equal(all[0].visits,3);assert.deepEqual(popularSites(rows,true).map(s=>s.site),['Pornhub']);
});
test('weekly extra sections depend on opt-in and unknown genres lower coverage',()=>{
 assert.deepEqual(extraSlides({config:{}}),[]);assert.deepEqual(extraSlides({config:{music_genres:true,adult_stats:true}}),['genres','adult']);assert.equal(genreCoverage({music_seconds:100,identified_music_seconds:30}),30);assert.equal(genreCoverage({music_seconds:0}),0);
});
