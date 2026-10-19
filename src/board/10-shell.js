/* SNAPSHOT */
const FILE_ICON='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 13h8 M8 17h5';
let ws = new URLSearchParams(location.search).get('workspace') || '', data = null, viewerActor = '';
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
// Until the workspace list arrives, the drop-down shows the last opened title, not a placeholder.
try{const title=sessionStorage.getItem('joppa-workspace-title');if(title)$('workspaces').append(E('option',title));}catch{}
const theme=$('theme');
theme.onclick=()=>{
  const next=darkTheme()?'light':'dark';
  document.documentElement.dataset.theme=next;
  try{localStorage.setItem('theme',next);}catch{}
  updateThemeColor();
};
const systemTheme=window.matchMedia('(prefers-color-scheme: dark)');
function darkTheme(){
  const selected=document.documentElement.dataset.theme;
  return selected==='dark'||selected==='system'&&systemTheme.matches;
}
function updateThemeColor(){
  const dark=darkTheme();
  theme.dataset.dark=String(dark);
  theme.title=dark?'Switch to light theme':'Switch to dark theme';
  theme.setAttribute('aria-label',theme.title);
  document.querySelector('meta[name="theme-color"]').content=dark?'#191c20':'#fffcf5';
}
systemTheme.addEventListener('change',updateThemeColor);
updateThemeColor();
function closeAccountMenu(){$('account-menu').open=false;}
document.addEventListener('click',event=>{if(!$('account-menu').contains(event.target))closeAccountMenu();},true);
window.addEventListener('popstate',()=>{
  closeAccountMenu();
  // The first entry has no workspace in its address. Its history state names the workspace.
  const next=new URLSearchParams(location.search).get('workspace')||history.state?.workspace||ws;
  if(next&&next!==ws){ws=next;$('workspaces').value=ws;clearBoard();}
  if(ws)load().catch(notify);
});
window.addEventListener('pageshow',closeAccountMenu);
document.addEventListener('keydown',event=>{
  if(event.key==='Escape'&&$('account-menu').open){$('account-menu').open=false;$('identity').focus();}
});
let toastTimer;
function toast(message,failed=false){
  let host=document.querySelector('#toast');
  if(!host){host=E('div',undefined,'toast');host.id='toast';host.setAttribute('role','status');host.setAttribute('aria-live','polite');document.body.append(host);}
  host.textContent=message;
  host.classList.toggle('toast-failed',failed);
  host.classList.remove('toast-leaving');
  // Restart the clock so the newest message always gets its full three seconds.
  void host.offsetWidth;host.classList.add('toast-visible');
  clearTimeout(toastTimer);
  toastTimer=setTimeout(()=>{host.classList.remove('toast-visible');host.classList.add('toast-leaving');},reducedMotion()?3000:3180);
  return host;
}
/// One copy action for every card. The label changes only after the clipboard
/// actually accepted the text, and the button keeps its size so the card never
/// moves.
async function copyAction(trigger,text){
  if(!text){toast('Nothing to copy yet. Reload the page.',true);return false;}
  let copied=false;
  try{
    if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
    await navigator.clipboard.writeText(text);
    copied=true;
  }catch{
    const field=E('textarea',text);field.style.position='fixed';field.style.opacity='0';field.style.top='0';
    document.body.append(field);field.select();
    try{copied=document.execCommand('copy');}catch{copied=false;}
    field.remove();
  }
  if(!copied){
    toast('Could not copy the ID. Select it in the dialog.',true);
    const dialog=E('dialog',undefined,'copy-dialog');dialog.setAttribute('aria-label','Copy ID');
    const field=E('input');field.value=text;field.readOnly=true;field.setAttribute('aria-label','ID');
    dialog.append(E('h2','Copy ID'),E('p','Select the ID. Use your device copy command.'),field,button('Close',()=>dialog.close()));
    dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();field.focus();field.select();
    return false;
  }
  trigger.classList.add('copied');trigger.dataset.copyState='copied';
  trigger.setAttribute('aria-label','Copied ID for agent');trigger.title='Copied ID for agent';
  clearTimeout(trigger.copyTimer);
  trigger.copyTimer=setTimeout(()=>{if(trigger.isConnected){trigger.classList.remove('copied');trigger.dataset.copyState='ready';trigger.setAttribute('aria-label','Copy ID for agent');trigger.title='Copy ID for agent';}},3000);
  toast('Copied '+text);
  return true;
}
function lineIcon(paths){
  const ns='http://www.w3.org/2000/svg',svg=document.createElementNS(ns,'svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'14',height:'14',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))svg.setAttribute(key,value);
  for(const d of paths){const path=document.createElementNS(ns,'path');path.setAttribute('d',d);svg.append(path);}
  return svg;
}
const historyIcon=()=>lineIcon(['M3 12a9 9 0 1 0 3-6.7','M3 4v4h4','M12 7v5l3 2']);
const propertiesIcon=()=>lineIcon(['M21 12a9 9 0 1 1-18 0a9 9 0 0 1 18 0','M12 16v-5','M12 8h.01']);
function copyButton(text){
  const control=button('',()=>copyAction(control,text),'copy');control.type='button';control.dataset.copyState='ready';
  control.title='Copy ID for agent';control.setAttribute('aria-label','Copy ID for agent');
  const ns='http://www.w3.org/2000/svg';
  const icon=(className,width)=>{const svg=document.createElementNS(ns,'svg');for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width,height:width,fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))svg.setAttribute(key,value);svg.classList.add(className);return svg;};
  const part=(tag,attributes)=>{const node=document.createElementNS(ns,tag);for(const [key,value] of Object.entries(attributes))node.setAttribute(key,value);return node;};
  const copy=icon('copy-symbol','14');copy.append(part('rect',{x:'8',y:'8',width:'12',height:'12',rx:'2'}),part('path',{d:'M16 8V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h4'}));
  const check=icon('copy-check','14');check.append(part('path',{d:'m5 12 4 4L19 6'}));
  const face=icon('agent-face','18');
  for(const [tag,attributes] of [['rect',{x:'4',y:'7',width:'16',height:'13',rx:'4'}],['path',{d:'M12 7V4m-2 0h4M1 12v4m22-4v4M9 16h6'}],['circle',{cx:'8.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}],['circle',{cx:'15.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}]])face.append(part(tag,attributes));
  control.append(copy,check,face);control.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();});return control;
}
function notify(e) {
  toast(e?.message||String(e||''),e instanceof Error);
}
async function api(path) {
  // A 404 changes the board only while the request still belongs to the shown selection.
  const generation=loadNumber,selected=ws;
  const r = await fetch('/api/' + path);
  // A proxy error page or a cut reply is not JSON. Report the HTTP status, not a parser error.
  const text = await r.text();
  let j;
  try{j=text?JSON.parse(text):{};}catch{j={error:'The server sent an unreadable reply (HTTP '+r.status+'). Try again.'};if(r.ok)throw Error(j.error);}
  if(r.status===401){loadNumber++;data=null;document.body.classList.remove('authenticated');$('tree').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')&&!path.includes('/objects?')&&generation===loadNumber&&selected===ws){data=null;$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));}
  if(path.includes('/objects?')&&(r.status===404||j.error==='unknown object'))throw Error('Object not found.');
  if (!r.ok) throw Error(j.error || JSON.stringify(j));
  return j;
}
function button(text, fn, cls = '') {
  const b = E('button', text, cls);
  b.type = 'button';
  b.onclick = () => {try{return Promise.resolve(fn()).catch(notify);}catch(e){notify(e);}};
  return b;
}
async function list() {
  const list = await api('workspaces');
  $('workspaces').replaceChildren();
  // Выбирать не из чего — значит и выбора нет. При одном воркспейсе список
  // остаётся видимым (человек должен знать, где он), но перестаёт быть
  // элементом управления: щелчок по нему не обещает того, чего не случится.
  $('workspaces').disabled = list.length <= 1;
  if (!list.length) {const o=E('option','No workspaces yet');o.value='';$('workspaces').append(o);}
  for (const w of list) {
    const o = E('option', w.title);
    o.value = w.id;
    $('workspaces').append(o);
  }
  const requested=new URLSearchParams(location.search).get('workspace');
  if(requested&&!list.some(w=>w.id===requested)){
    ws='';data=null;$('workspaces').value='';
    const link=E('a','Back to board','text-button');const url=new URL(location.href);url.search='';url.hash='';link.href=url.href;
    $('tree').replaceChildren(E('p','This workspace is not available to your account.','empty-state'),link);return;
  }
  if (!list.some(w => w.id === ws)) ws = list.find(w=>w.id===sessionStorage.getItem('joppa-workspace'))?.id || list[0]?.id || '';
  $('workspaces').value = ws;
  // Back to the first entry returns to the workspace that the list chose for it.
  if(ws&&!requested)history.replaceState({...history.state,workspace:ws},'',location.href);
  try{sessionStorage.setItem('joppa-workspace-title',list.find(w=>w.id===ws)?.title||'');}catch{}
  if (ws) await load();
  else {data=null;$('tree').replaceChildren(E('h1','No workspace access'),E('p','Ask your workspace owner to grant access.'));}
}
