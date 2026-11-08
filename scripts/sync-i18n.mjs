import {readFile,writeFile} from 'node:fs/promises';import {messages} from '../src/translations.mjs';
const root=new URL('../',import.meta.url);const check=process.argv.includes('--check');
for(const name of ['i18n.mjs','translations.mjs']){const content=await readFile(new URL('src/'+name,root));const target=new URL('browser-extension/'+name,root);if(check){if(!(await readFile(target)).equals(content))throw Error('Stale browser translation: '+name);}else await writeFile(target,content);}
const path=new URL('src/translations.json',root);if(check){if(JSON.stringify(JSON.parse(await readFile(path,'utf8')))!==JSON.stringify(messages))throw Error('Stale native translation catalog');}else await writeFile(path,JSON.stringify(messages));
