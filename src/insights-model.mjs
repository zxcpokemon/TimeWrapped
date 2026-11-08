import {t,tr,localize,locale} from './i18n.mjs';
export function popularSites(rows,adultOnly=false){const map=new Map();for(const row of rows||[]){if(adultOnly&&!row.adult)continue;const entry=map.get(row.site)||{site:row.site,adult:!!row.adult,seconds:0,visits:0};entry.seconds+=Number(row.seconds)||0;entry.visits+=Number(row.visits)||0;map.set(row.site,entry);}return [...map.values()].sort((a,b)=>b.seconds-a.seconds);}
export function genreCoverage(report){return report?.music_seconds>0?Math.max(0,Math.min(100,100*(report.identified_music_seconds||0)/report.music_seconds)):0;}
export function extraSlides(report){const slides=[];if(report?.config?.music_genres)slides.push('genres');if(report?.config?.adult_stats)slides.push('adult');return slides;}
