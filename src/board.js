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
window.addEventListener('popstate',closeAccountMenu);
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
  const r = await fetch('/api/' + path);
  const j = await r.json();
  if(r.status===401){loadNumber++;data=null;document.body.classList.remove('authenticated');$('tree').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')&&!path.includes('/objects?')){data=null;$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));}
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
  if (ws) await load();
  else {data=null;$('tree').replaceChildren(E('h1','No workspace access'),E('p','Ask your workspace owner to grant access.'));}
}
let loadNumber = 0;
const branchAnimations=new WeakMap();
const reducedMotion=()=>window.matchMedia('(prefers-reduced-motion: reduce)').matches;
function stopBranchAnimation(node){
  branchAnimations.get(node)?.cancel();
  branchAnimations.delete(node);
  node.style.height='';node.style.overflow='';
}
function animateBranchHeight(node,from,to,closing=false){
  if(reducedMotion()||Math.abs(to-from)<2){
    stopBranchAnimation(node);
    if(closing){node.open=false;node.classList.remove('branch-closing');}
    return;
  }
  node.style.height=from+'px';node.style.overflow='clip';
  const animation=node.animate([{height:from+'px'},{height:to+'px'}],{
    duration:220,easing:'cubic-bezier(.2,.75,.25,1)'
  });
  branchAnimations.set(node,animation);
  animation.onfinish=()=>{
    if(branchAnimations.get(node)!==animation)return;
    stopBranchAnimation(node);
    if(closing){node.open=false;node.classList.remove('branch-closing');}
  };
}
// Opened content arrives softly instead of appearing at once.
function revealBranch(node){
  const body=node.querySelector(':scope > .object-body');
  if(!body||reducedMotion())return;
  body.animate([{opacity:0,transform:'translateY(-4px)'},{opacity:1,transform:'none'}],{duration:260,easing:'cubic-bezier(.2,.75,.25,1)'});
}
function closedBranchHeight(node){
  const style=getComputedStyle(node),summary=node.querySelector(':scope > summary');
  return summary.getBoundingClientRect().height+
    parseFloat(style.paddingTop)+parseFloat(style.paddingBottom)+
    parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth);
}
function enableBranchMotion(node){
  node.querySelector(':scope > summary').addEventListener('click',event=>{
    if(event.defaultPrevented||event.target.closest('button,a,input,label,select,textarea'))return;
    event.preventDefault();
    const opening=!node.open||node.classList.contains('branch-closing');
    // A branch with lazy content loads while closed, then opens once to its full height.
    if(opening&&!node.open&&node.branchPrepare&&!node.branchReady){
      if(node.classList.contains('is-loading'))return;
      node.classList.add('is-loading');
      node.branchPrepare().then(()=>{
        node.classList.remove('is-loading');node.branchReady=true;
        const start=node.getBoundingClientRect().height;node.open=true;
        animateBranchHeight(node,start,node.getBoundingClientRect().height);revealBranch(node);
      },()=>node.classList.remove('is-loading'));
      return;
    }
    const from=node.getBoundingClientRect().height;
    stopBranchAnimation(node);
    node.classList.toggle('branch-closing',!opening);
    if(opening)node.open=true;
    if(opening&&node.dataset.pending==='true')return;
    const to=opening?node.getBoundingClientRect().height:closedBranchHeight(node);
    animateBranchHeight(node,from,to,!opening);
  });
}
function resizeBranch(node,update){
  const before=node.getBoundingClientRect().height;
  for(let current=node;current;current=current.parentElement?.closest('details')){
    if(!current.classList.contains('branch-closing'))stopBranchAnimation(current);
  }
  update();
  if(node.open&&!node.classList.contains('branch-closing'))animateBranchHeight(node,before,node.getBoundingClientRect().height);
}
async function load() {
  closeAccountMenu();
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)+'/board'));
  if (request !== loadNumber) return;
  data = next;
  data.objectTypes=next.object_types||{};
  $('tree-schema-link').href='/tree?workspace='+encodeURIComponent(ws);
  $('suggested-method-link').href='/guide?workspace='+encodeURIComponent(ws);
  await renderObjectTree();
}
async function showApp(identity) {
  closeAccountMenu();
  if(identity.email===undefined)identity={...identity,...await api('me')};
  viewerActor=identity.actor;
  $('identity').textContent=displayPersonName(identity.name||identity.email||identity.actor||'Account');
  $('identity').title=identity.email||'';
  document.body.classList.add('authenticated');
  $('logout').hidden=false;
  await list();
}
let signInAttempt = 0;
let signInWindow = null;
$('login-cancel').onclick=()=>{
  signInAttempt++;signInWindow?.close();$('login-button').disabled=false;
  $('login-cancel').hidden=true;$('login-status').textContent='';
};
$('login-button').onclick=async()=>{
  const attempt=++signInAttempt;
  const b=$('login-button'), status=$('login-status');b.disabled=true;status.dataset.error='false';
  status.textContent='Opening Google…';
  // Open synchronously from the click so browsers do not block the sign-in window.
  signInWindow=window.open('about:blank','joppa-google','popup,width=520,height=700');
  if(signInWindow)signInWindow.opener=null;
  try {
    const r=await fetch('/auth/login',{method:'POST'});const start=await r.json();
    if(!r.ok)throw Error(start.error||'Could not start sign-in.');
    if(attempt!==signInAttempt)return;
    if(!signInWindow)throw Error('Allow the Google sign-in window, then try again.');
    signInWindow.location=start.verification_url;
    status.textContent='Complete sign-in in the Google window.';$('login-cancel').hidden=false;
    const deadline=Date.now()+start.expires_in*1000;
    while(attempt===signInAttempt && Date.now()<deadline){
      await new Promise(resolve=>setTimeout(resolve,1800));
      if(attempt!==signInAttempt)return;
      const response=await fetch('/auth/poll',{method:'POST'}), result=await response.json();
      if(attempt!==signInAttempt)return;
      if(!response.ok)throw Error(result.error||'Sign-in failed. Please try again.');
      if(result.status==='ready'){signInWindow?.close();await showApp(result);return;}
      // Google may sever the popup reference via COOP; the server decides completion.
    }
    if(attempt===signInAttempt)throw Error('Sign-in expired. Please try again.');
  }catch(e){if(attempt===signInAttempt){status.textContent=e.message;status.dataset.error='true';signInWindow?.close();}}
  finally{if(attempt===signInAttempt){b.disabled=false;$('login-cancel').hidden=true;}}
};
$('logout').onclick=async()=>{await fetch('/auth/logout',{method:'POST'});sessionStorage.removeItem('joppa-token');location.reload();};
$('workspaces').onchange=async e=>{
  ws=e.target.value;sessionStorage.setItem('joppa-workspace',ws);
  location.assign(objectURL());
};
$('refresh').onclick = () => load().catch(notify);
let statusHint=null;
function hideStatusHint(){if(statusHint)statusHint.hidden=true;}
function showStatusHint(circle){
  if(!statusHint){statusHint=E('div',undefined,'status-hint');statusHint.hidden=true;document.body.append(statusHint);}
  statusHint.textContent=circle.dataset.hint;
  statusHint.hidden=false;
  const dot=circle.getBoundingClientRect(),box=statusHint.getBoundingClientRect();
  const left=Math.max(8,Math.min(dot.left+dot.width/2-box.width/2,window.innerWidth-box.width-8));
  const below=dot.bottom+10;
  const top=below+box.height<=window.innerHeight-8?below:Math.max(8,dot.top-box.height-10);
  statusHint.style.left=left+'px';statusHint.style.top=top+'px';
}
document.addEventListener('scroll',hideStatusHint,true);
function statusCircle(signal,number){
  const circle=E('span',number===undefined?undefined:String(number),'status-circle '+signal.tone+(number===undefined?'':' numbered'));
  const assessment=signal.readiness_blocker||signal.assessment;
  const hint='Status: '+signal.label+assessmentHint(assessment)+(signal.at?'\nStatus event: '+eventDate(signal.at):'');
  circle.dataset.tone=signal.tone;circle.dataset.hint=hint;circle.tabIndex=0;
  circle.setAttribute('role','img');circle.setAttribute('aria-label',hint);
  circle.addEventListener('pointerenter',()=>showStatusHint(circle));
  circle.addEventListener('pointerleave',hideStatusHint);
  circle.addEventListener('focus',()=>showStatusHint(circle));
  circle.addEventListener('blur',hideStatusHint);
  return circle;
}
function assessmentHint(a){
  if(!a)return '';
  if(a.state==='unassessed')return '\nReadiness: Not assessed';
  if(a.state==='waiting')return '\nQuestion: '+a.reason+'\nRecipient: '+personLabel(a.recipient||'Unassigned');
  return '\nReadiness: Ready';
}
function nodeTitle(text,signal){
  const title=E('span',text,'node-title');title.prepend(statusCircle(signal));return title;
}
function rootCollectionTabs(root){
  if(!Object.keys(data.objectTypes||{}).length)return;
  const types=Object.entries(data.objectTypes||{}).filter(([,type])=>type.root)
    .sort(([a,left],[b,right])=>(left.index??Infinity)-(right.index??Infinity)||a.localeCompare(b));
  const existing=[...root.querySelectorAll('.root-collection')];
  for(const panel of existing)panel.hidden=true;
  if(!types.length){root.querySelector(':scope > .collection-tabs')?.remove();return;}
  const panels=types.map(([kind,type])=>{
    let panel=existing.find(node=>node.dataset.collectionKind===kind);
    if(!panel){
      panel=E('section',undefined,'root-collection');panel.dataset.collectionKind=kind;panel.dataset.collectionLazy='true';applyObjectColor(panel,kind);
      panel.append(E('header',undefined,'collection-heading'),E('div',undefined,'collection-items'));root.append(panel);
      collectionHeading(panel,type.title,type.description||'');
    }
    return panel;
  });
  let nav=root.querySelector(':scope > .collection-tabs');
  if(!nav){nav=E('div',undefined,'collection-tabs');nav.setAttribute('role','tablist');nav.setAttribute('aria-label','Root collections');root.prepend(nav);}
  nav.replaceChildren();nav.hidden=false;
  const key='joppa-collection:'+ws;
  let selected;
  try{selected=sessionStorage.getItem(key);}catch{}
  if(!panels.some(panel=>panel.dataset.collectionKind===selected))selected=panels[0]?.dataset.collectionKind;
  const activate=(kind,focus=false)=>{
    closeAccountMenu();
    selected=kind;try{sessionStorage.setItem(key,kind);}catch{}
    for(const panel of panels){
      const active=panel.dataset.collectionKind===kind;panel.hidden=!active||Boolean(data.filterActive);
      const tab=nav.querySelector('[aria-controls="'+panel.id+'"]');tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;
      if(active){
        data.activeRootKind=kind;
        if(focus)tab.focus();
        if(data.filterActive)continue;
        panel.collectionLoad?.();
        if(panel.dataset.collectionLazy==='true'){
          delete panel.dataset.collectionLazy;
          const contents=panel.querySelector(':scope > .collection-items')||panel;
          contents.append(E('p','Loading objects…','object-loading hint'));
          appendObjectChildren(contents,undefined,kind).catch(error=>{panel.dataset.collectionLazy='true';contents.replaceChildren();notify(error);});
        }
      }
    }
    if(data.filterActive)data.filterUpdate?.();
  };
  for(const panel of panels){
    const kind=panel.dataset.collectionKind,type=data.objectTypes[kind];
    panel.id='collection-'+kind;panel.setAttribute('role','tabpanel');panel.setAttribute('aria-labelledby','tab-'+kind);panel.tabIndex=-1;
    const tab=button(type.title,()=>activate(kind),'collection-tab');applyObjectColor(tab,kind);tab.id='tab-'+kind;tab.setAttribute('role','tab');tab.setAttribute('aria-controls',panel.id);nav.append(tab);
  }
  nav.onkeydown=event=>{
    const tabs=[...nav.children],current=tabs.indexOf(document.activeElement);let next;
    if(event.key==='ArrowRight')next=(current+1)%tabs.length;
    if(event.key==='ArrowLeft')next=(current+tabs.length-1)%tabs.length;
    if(event.key==='Home')next=0;if(event.key==='End')next=tabs.length-1;
    if(next!==undefined){event.preventDefault();activate(panels[next].dataset.collectionKind,true);}
  };
  activate(selected);
}
function collectionHeading(section,title,description,label){
  const summary=section.querySelector(':scope > summary,:scope > header');
  const caption=E('span',undefined,'collection-caption');
  caption.append(E('span',title,'collection-title'),E('span',description,'collection-description'));
  summary.replaceChildren(caption);
  if(label)summary.append(E('span',label,'collection-label'));
}
function displayPersonName(value){
  const name=String(value||'').trim();
  if(!name)return 'Not assigned';
  const local=name.includes('@')?name.split('@')[0].split('+')[0]:name;
  if(!/^[\p{L}]+(?:[._][\p{L}]+)+$/u.test(local))return local;
  return local.split(/[._]/).map(part=>part[0].toLocaleUpperCase()+part.slice(1)).join(' ');
}
function personLabel(actor){
  return displayPersonName(data.member_labels?.[actor]||actor);
}
function eventDate(value){
  const date=new Date(value.replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isNaN(date.getTime())?'Date unavailable':new Intl.DateTimeFormat('en-GB',{day:'numeric',month:'short',year:'numeric',hour:'2-digit',minute:'2-digit'}).format(date);
}
if (window.SNAPSHOT) {
  document.body.classList.remove('checking-session');
  document.body.classList.add('snapshot');
  $('login-screen').remove();
  $('refresh').remove();
  document.body.classList.add('authenticated');
  data = window.SNAPSHOT;
  ws = data.workspace;
  const option=E('option',data.state.title);option.value=ws;$('workspaces').replaceChildren(option);
  load().catch(notify);
}

// Browser sessions use an HttpOnly cookie; no token field or token URL.
if(!window.SNAPSHOT){
  sessionStorage.removeItem('joppa-token');
  if(location.hash.startsWith('#token='))history.replaceState(null,'',location.pathname+location.search);
  fetch('/api/me').then(async r=>{
    if(r.ok)await showApp(await r.json());
    else if(r.status!==401)throw Error('Could not restore your session. Please reload the page.');
  }).catch(e=>{$('login-status').textContent=e.message;$('login-status').dataset.error='true';})
    .finally(()=>document.body.classList.remove('checking-session'));
}
function when(at){
  if(!at)return '';
  const d=new Date(at.replace(' ','T').replace(/\+00$/,'Z'));
  return isNaN(d)?at:d.toLocaleString(undefined,{dateStyle:'medium',timeStyle:'short'});
}
function svgIcon(d,size=16){
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [k,v] of Object.entries({viewBox:'0 0 24 24',width:String(size),height:String(size),fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(k,v);
  const path=document.createElementNS(icon.namespaceURI,'path');path.setAttribute('d',d);icon.append(path);
  return icon;
}
// Кнопка с цифрой. Цифра — это и есть ответ на вопрос «тут что-то обсуждали?»,
// ради которого иначе пришлось бы открывать каждый объект по очереди.
function attachmentList(attachments){
  const list=E('ul',undefined,'files-list');
  for(const [,a] of attachments){
    const li=E('li'),link=E('a',undefined,'file-link');
    link.href='/api/workspaces/'+encodeURIComponent(ws)+'/blobs/'+a.hash;
    link.target='_blank';link.rel='noopener';
    const icon=E('span',undefined,'file-icon');icon.append(svgIcon(FILE_ICON,22));
    const info=E('span',undefined,'file-info');
    info.append(E('span',a.name,'file-name'),E('span',[fileSize(a.size),personLabel(a.actor),when(a.at)].filter(Boolean).join(' · '),'file-meta'));
    link.append(icon,info,svgIcon('M7 17 17 7 M7 7h10v10',16));
    li.append(link);list.append(li);
  }
  return list;
}
function fileSize(bytes){
  if(bytes<1024)return bytes+' B';
  if(bytes<1024*1024)return Math.round(bytes/1024)+' KB';
  return (bytes/(1024*1024)).toFixed(1)+' MB';
}
/* Common objects use the same cards for every configured type. */
async function objectPart(params){
  if(!window.SNAPSHOT)return api('workspaces/'+encodeURIComponent(ws)+'/objects?'+new URLSearchParams(params));
  const objects=data.snapshot_objects||{};
  if(params.address){
    const object=objects[params.address];if(!object)throw Error('Unknown object');
    const document=object.revisions.find(r=>r.revision===Number(params.revision||object.current_revision));if(!document)throw Error('Unknown revision');
    const historical=document.revision!==object.current_revision;
    const links=document.links.map(link=>({link,title:objects[link.target]?.revisions.at(-1).data.title,current_revision:objects[link.target]?.current_revision}));
    return {...object,document,historical,links,annotations:historical?{}:object.annotations,process:historical?null:object.process};
  }
  const presentParents=new Set(Object.values(objects).filter(o=>o.revisions.at(-1).present).map(o=>o.revisions.at(-1).parent));
  const items=Object.values(objects).filter(o=>{
    const r=o.revisions.at(-1);
    const related=params.related_to&&(r.parent===params.related_to||r.links.some(l=>l.target===params.related_to)||objects[params.related_to]?.revisions.at(-1).links.some(l=>l.target===o.address));
    return (r.present||presentParents.has(o.address))&&(!params.kind||params.kind===o.kind)&&(params.related_to?related:params.parent==='*'||(params.parent||null)===r.parent)&&(!params.note_status||(r.data.status||'unprocessed')===params.note_status);
  }).map(o=>{const r=o.revisions.at(-1);return {...o,parent:r.parent,title:r.data.title,owner:r.data.owner,revision:r.revision,at:r.at,present:r.present,note_status:r.data.status};});
  return {items,next_cursor:null};
}
function applyObjectColor(node,kind){
  const type=data.objectTypes?.[kind]||{};
  const color=type.color||'#63756b';
  if(/^#[0-9a-f]{6}$/i.test(color))node.style.setProperty('--object-color',color);
  const size=type.title_size;
  node.style.setProperty('--object-title-size',(Number.isInteger(size)&&size>=12&&size<=48?size:18)+'px');
}
function objectURL(address,revision){
  const url=new URL(location.href);url.search='';url.hash='';
  url.searchParams.set('workspace',ws);
  if(address)url.searchParams.set('object',address);
  if(revision)url.searchParams.set('revision',revision);
  return url.href;
}
function objectTitlePreview(title,kind){
  const limit=data.objectTypes?.[kind]?.title_preview_length||64,characters=Array.from(title);
  return characters.length>limit?characters.slice(0,limit-1).join('').trimEnd()+'…':title;
}
function objectLink(title,address,revision){
  const link=E('a',objectTitlePreview(title,address.split(':')[0]),'object-link');link.setAttribute('aria-label',title);link.href=objectURL(address,revision);link.dataset.linkAddress=address;return link;
}
async function renderObjectPage(root,address,revision,generation){
  const navigation=E('nav',undefined,'object-page-navigation');navigation.setAttribute('aria-label','Object navigation');
  const board=E('a','Back to board','text-button');board.href=objectURL();navigation.append(board);root.append(navigation);
  const page=E('section',undefined,'object-page');root.append(page);page.append(E('p','Loading object…','hint'));
  try{
    if(!address)throw Error('Object not found.');
    if(revision!==null&&!/^[1-9]\d*$/.test(revision))throw Error('Invalid object revision.');
    const detail=await objectPart(revision===null?{address}:{address,revision});if(generation!==loadNumber)return;
    const r=detail.document,d=r.data;
    document.title=d.title+' · Joppa';
    navigation.append(E('span','Revision '+r.revision+(detail.historical?' · Saved document':''),'hint'));
    if(r.parent)navigation.append(objectLink('Parent',r.parent));
    const card=commonObjectCard({...detail,title:d.title,owner:d.owner,revision:r.revision,parent:r.parent,present:r.present,at:r.at,note_status:d.status},detail);
    page.replaceChildren(card);card.open=true;
  }catch(e){if(generation===loadNumber&&page.isConnected)page.replaceChildren(E('p',e.message,'empty-state'));}
}
async function appendObjectChildren(root,parent,kind,extra={}){
  const generation=loadNumber;
  const params={limit:'50',...extra};if(parent)params.parent=parent;if(kind)params.kind=kind;
  const part=await objectPart(params);if(generation!==loadNumber||!root.isConnected)return;
  root.querySelector(':scope > .object-loading')?.remove();
  appendObjectPage(root,part,params,generation);
}
function appendObjectPage(root,part,params,generation){
  const items=part.items;
  const ancestors=new Set();
  for(let node=root;node;node=node.parentElement){
    const address=node.dataset.objectAddress||node.dataset.objectContextAddress;
    if(address)ancestors.add(address);
  }
  for(const item of items){
    if(ancestors.has(item.address))continue;
    let holder=root;
    if(!params.parent&&!params.kind){
      holder=[...root.querySelectorAll(':scope > .object-collection')].find(group=>group.dataset.kind===item.kind);
      if(!holder){
        holder=E('section',undefined,'object-collection root-collection');holder.dataset.kind=item.kind;holder.dataset.collectionKind=item.kind;applyObjectColor(holder,item.kind);
        holder.append(E('h2',data.objectTypes[item.kind].title,'object-collection-title'));root.append(holder);
      }
    }
    if(![...holder.querySelectorAll(':scope > .common-object')].some(card=>card.dataset.objectAddress===item.address)){
      const card=commonObjectCard(item,undefined,root.closest('.common-object')?.dataset.objectTitle);holder.append(card);
      if(!new URLSearchParams(location.search).has('object')&&openBranches().has(item.address))card.open=true;
    }
  }
  applyTestStatuses(items);
  if(!items.length&&!part.next_cursor&&!params.parent&&!root.querySelector('.common-object')){
    const type=params.kind&&data.objectTypes[params.kind];
    root.append(E('p',type?'No '+type.title.toLowerCase()+' here.':'No objects here yet.','hint collection-empty'));
  }
  if(part.next_cursor){const more=button('Load more',async()=>{more.disabled=true;try{const next=await objectPart({...params,cursor:part.next_cursor});if(generation!==loadNumber||!root.isConnected)return;more.remove();appendObjectPage(root,next,params,generation);}catch(e){more.disabled=false;notify(e);}},'text-button');root.append(more);}
  if(!params.parent&&!params.kind&&$('tree').querySelector('.collection-tabs'))rootCollectionTabs($('tree'));
}
async function renderObjectTree(){
  const root=$('tree');root.replaceChildren();
  const generation=loadNumber;
  const configuration=window.SNAPSHOT?{schema:data.snapshot_schema}:await api('workspaces/'+encodeURIComponent(ws)+'/tree');if(generation!==loadNumber)return;
  data.objectTypes=configuration.schema.types;
  const selection=new URLSearchParams(location.search);
  if(selection.has('object'))return renderObjectPage(root,selection.get('object'),selection.get('revision'),generation);
  if(data.launch.configured)root.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(data.launch.blocker_count??data.launch.blockers?.length),'hint'));
  const contents=E('div',undefined,'object-filter-results');contents.hidden=true;
  commonObjectFilters(root,contents);root.append(contents);
  for(const kind of inboxKinds().filter(kind=>data.objectTypes[kind].root))commonNotes(root,undefined,kind);
  rootCollectionTabs(root);
}
/// Every configured inbox type gets its own collection, in schema order.
function inboxKinds(){
  return Object.entries(data.objectTypes||{}).filter(([,t])=>t.inbox===true)
    .sort((a,b)=>(a[1].index??Infinity)-(b[1].index??Infinity)||a[0].localeCompare(b[0]))
    .map(([kind])=>kind);
}
/// An input queue shows unprocessed entries first. The schema declares it.
function inputQueue(kind){
  return kind?data.objectTypes[kind]?.inbox===true:false;
}
function commonObjectFilters(root,contents){
  const controls=E('div',undefined,'filter-fields');let request=0;
  const picker=(label,options)=>{const select=E('select');select.setAttribute('aria-label',label);for(const [value,title] of options){const option=E('option',title);option.value=value;select.append(option);}controls.append(select);return select;};
  const owner=picker('Object owner',[['','All owners'],...Object.keys(data.state.members).map(id=>[id,personLabel(id)])]);
  const status=picker('Object status',[['','All statuses'],['green','Checks passed'],['red','Checks failed'],['purple','Dependency review'],['yellow','Waiting or in progress'],['grey','Unchecked or completed']]);
  const update=async()=>{const mine=++request,generation=loadNumber;const selected={kind:data.activeRootKind,owner:owner.value,status:status.value};const filtering=Boolean(selected.owner||selected.status);data.filterActive=filtering;contents.hidden=!filtering;const holder=E('div');contents.replaceChildren(holder);try{
    if(!filtering){rootCollectionTabs(root);return;}
    for(const panel of root.querySelectorAll('.root-collection'))panel.hidden=true;
    holder.append(E('p','Loading objects…','object-loading hint'));
    const params={parent:'*',limit:'100'};if(selected.kind)params.kind=selected.kind;if(inputQueue(selected.kind))params.note_status='unprocessed';let cursor;const items=[];
    do{const page=await objectPart(cursor?{...params,cursor}:params);if(mine!==request||generation!==loadNumber||!controls.isConnected)return;items.push(...page.items);cursor=page.next_cursor;}while(cursor);
    const rows=items.filter(item=>(selected.kind||!inputQueue(item.kind))&&(!selected.owner||item.owner===selected.owner)&&(!selected.status||item.signal?.tone===selected.status));
    holder.replaceChildren(...rows.map(commonObjectCard));if(!rows.length)holder.append(E('p','No objects match these filters.','hint'));
  }catch(e){if(mine===request)notify(e);}};
  data.filterActive=false;data.filterUpdate=update;
  for(const select of [owner,status])select.onchange=update;root.append(controls);return controls;
}

function commonNotes(root,relatedTo,kind){
  const label=data.objectTypes?.[kind]?.title||kind;
  const title=relatedTo?'Related '+label.toLowerCase():label;
  const queue=inputQueue(kind);
  const section=E('section');section.append(E('header',undefined,'collection-heading'));root.append(section);section.classList.add('note-section');applyObjectColor(section,kind);
  collectionHeading(section,title,data.objectTypes?.[kind]?.description||(!relatedTo&&queue?'Incoming information to review.':''));
  if(!relatedTo){section.classList.add('root-collection');section.dataset.collectionKind=kind;}
  const summary=section.querySelector(':scope > header');
  const body=E('div',undefined,'note-section-body');section.append(body);
  let loaded=false,loading=false,request=0;
  let all;
  if(!relatedTo&&queue){
    const controls=E('div',undefined,'note-controls');
    const option=E('label',undefined,'note-all');all=E('input');all.type='checkbox';all.setAttribute('aria-label','All '+label.toLowerCase());option.append(all,E('span','All'));controls.append(option);summary.append(controls);
  }
  const rows=E('div',undefined,'note-items');body.append(rows);
  const loadNotes=async()=>{
    const mine=++request;loaded=false;loading=true;
    const holder=E('div');holder.append(E('p','Loading '+label.toLowerCase()+'…','object-loading hint'));rows.replaceChildren(holder);
    try{await appendObjectChildren(holder,'*',kind,relatedTo?{related_to:relatedTo}:queue&&!all?.checked?{note_status:'unprocessed'}:{});if(mine===request&&section.isConnected){
      loaded=true;const empty=!holder.querySelector('.common-object');section.classList.toggle('empty-notes',empty);if(relatedTo)section.hidden=empty;
      summary.title=empty?'No '+label.toLowerCase()+' here.':'Read '+label.toLowerCase();
    }}
    catch(e){if(mine===request){loaded=false;holder.replaceChildren();notify(e);}}
    finally{if(mine===request)loading=false;}
  };
  if(all)all.onchange=loadNotes;
  section.collectionLoad=()=>{if(!loaded&&!loading)loadNotes();};
  if(relatedTo){
    const parent=section.parentElement.closest('details');
    if(!parent||parent.open)section.collectionLoad();
    else parent.addEventListener('toggle',()=>{if(parent.open)section.collectionLoad();});
  }
  return section;
}
function objectTypeLabel(kind){
  const type=data.objectTypes?.[kind]||{},title=type.title||kind.toUpperCase();
  const words=title.trim().split(/\s+/);
  const short=words.length===1&&title===title.toLocaleUpperCase()?title:words.map(word=>Array.from(word)[0]).join('').toLocaleUpperCase();
  const question=type.question||'',hint=question?title+'\n'+question:title;
  const label=E(question?'button':'abbr',type.badge||short,'type-label type-badge');
  label.dataset.hint=hint;label.title=hint;label.tabIndex=0;label.setAttribute('aria-label',title);
  if(question){
    label.type='button';label.setAttribute('aria-expanded','false');
    const panel=E('div',question,'type-question');panel.id='type-question-'+crypto.randomUUID();panel.setAttribute('popover','auto');
    label.setAttribute('aria-controls',panel.id);
    label.setAttribute('popovertarget',panel.id);
    label.addEventListener('click',event=>{
      event.stopPropagation();hideStatusHint();
      if(!panel.isConnected)document.body.append(panel);
    });
    panel.addEventListener('beforetoggle',event=>{
      label.setAttribute('aria-expanded',String(event.newState==='open'));
      if(event.newState==='open')panel.style.visibility='hidden';
    });
    panel.addEventListener('toggle',()=>{
      if(!panel.matches(':popover-open')){panel.remove();return;}
      const anchor=label.getBoundingClientRect(),box=panel.getBoundingClientRect();
      panel.style.left=Math.max(8,Math.min(anchor.left,window.innerWidth-box.width-8))+'px';
      panel.style.top=(anchor.bottom+10+box.height<=window.innerHeight-8?anchor.bottom+10:Math.max(8,anchor.top-box.height-10))+'px';
      panel.style.visibility='';
    });
  }
  label.addEventListener('pointerenter',()=>showStatusHint(label));
  label.addEventListener('pointerleave',hideStatusHint);
  label.addEventListener('focus',()=>showStatusHint(label));
  label.addEventListener('blur',hideStatusHint);
  return label;
}
// Open branches survive a visit to an object page and the way back to the board.
const openKey=()=>'joppa-open-'+ws;
function openBranches(){try{return new Set(JSON.parse(sessionStorage.getItem(openKey())||'[]'));}catch{return new Set();}}
function rememberBranch(address,open){
  try{const set=openBranches();if(open)set.add(address);else set.delete(address);sessionStorage.setItem(openKey(),JSON.stringify([...set]));}catch{}
}
// Object text is Markdown. Line breaks stay, active HTML is removed, and external links open safely.
const markdownSafe=(()=>{
  if(!window.DOMPurify||!window.marked)return null;
  DOMPurify.addHook('afterSanitizeAttributes',node=>{
    if(node.tagName==='A'){
      const href=node.getAttribute('href')||'';
      if(/^\s*(javascript|data|vbscript):/i.test(href))node.removeAttribute('href');
      else if(/^https?:/i.test(href)){node.setAttribute('target','_blank');node.setAttribute('rel','noopener noreferrer');}
    }
    if(node.tagName==='INPUT')node.setAttribute('disabled','');
  });
  return text=>DOMPurify.sanitize(marked.parse(text,{gfm:true,breaks:true}),
    {FORBID_TAGS:['style','iframe','svg','math','form','object','embed'],FORBID_ATTR:['style']});
})();
function markdownBlock(text,cls){
  const box=E('div',undefined,'markdown '+cls);
  if(markdownSafe)box.innerHTML=markdownSafe(text);else{box.textContent=text;box.classList.add('markdown-raw');}
  return box;
}
function commonObjectCard(item,loadedDetail,parentTitle){
  const card=E('details',undefined,'common-object');card.dataset.objectAddress=item.address;card.dataset.kind=item.kind;card.dataset.objectTitle=item.title;applyObjectColor(card,item.kind);
  const summary=E('summary',undefined,'title-row');
  const tone=item.review_required?'purple':item.work_ready?'grey':'yellow';
  const signal=item.historical?{tone:'grey',label:'Saved revision',at:item.at}:item.present===false?{tone:'grey',label:'Historical object',at:item.at}:item.signal?.tone?item.signal:{tone,label:item.review_required?'Dependency changed':item.work_ready?'Current document':'Dependency not ready',at:item.at};
  const repeatsParent=item.title===parentTitle;
  const heading=repeatsParent?data.objectTypes[item.kind].title+' · '+signal.label:item.title;
  const preview=objectTitlePreview(heading,item.kind);
  const title=nodeTitle(preview,signal),counts=item.verification_summary;title.dataset.truncated=String(!repeatsParent&&preview!==item.title);
  const isCriterion=data.objectTypes?.[item.kind]?.role==='ac';
  // The count sits on the right of the row, after the type label, not inside the title.
  let chip=null;
  if(isCriterion&&signal.tone==='green'){
    chip=E('span','Passed','verification-chip has-passes');
  }else if(!isCriterion&&counts&&verificationChildTitle(item.kind)){
    chip=E('span',undefined,'verification-chip');chip.dataset.address=item.address;chip.dataset.total=counts.total;chip.dataset.passed=counts.passed;chip.dataset.failed=counts.failed||0;chip.dataset.childTitle=verificationChildTitle(item.kind);showTestCount(chip);
  }
  // An empty slot keeps the count column aligned on rows without a count.
  summary.append(title,objectTypeLabel(item.kind),chip||E('span',undefined,'count-slot'));
  // The count can colour the row dot only once both sit in the summary.
  if(chip?.dataset.total)showTestCount(chip);
  if(inputQueue(item.kind))summary.append(E('span',item.note_status==='processed'?'Processed':'Unprocessed','note-state'));
  const actions=E('span',undefined,'item-actions');
  const copy=copyButton(item.item_id||item.address);
  const history=button('',()=>showObjectRevisions(item.address),'history-button');history.type='button';
  history.title='Revisions';history.setAttribute('aria-label','Revisions');history.append(historyIcon());
  history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  // The properties open and close from the row. The object stays open or closed.
  const fieldsToggle=button('',()=>{
    const shown=fieldsToggle.getAttribute('aria-expanded')!=='true';fieldsToggle.setAttribute('aria-expanded',String(shown));
    const fields=body.querySelector(':scope > .object-secondary > .object-details');if(fields)resizeBranch(card,()=>{fields.hidden=!shown;});
  },'fields-button');
  fieldsToggle.type='button';fieldsToggle.title='Properties';fieldsToggle.setAttribute('aria-label','Properties');fieldsToggle.append(propertiesIcon());
  fieldsToggle.setAttribute('aria-expanded','false');fieldsToggle.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  actions.append(fieldsToggle,copy,history);summary.append(actions);card.append(summary);
  const body=E('div',undefined,'object-body');card.append(body);enableBranchMotion(card);let loaded=false;
  // Fill the body with the document and its children. Both requests run at once.
  const fill=async()=>{
    const generation=loadNumber,childParams={limit:'50',parent:item.address};
    const [detail,kids]=await Promise.all([loadedDetail||objectPart({address:item.address}),objectPart(childParams).catch(()=>null)]);
    if(generation!==loadNumber||!card.isConnected)return;
    body.replaceChildren();fillCommonObject(body,detail);
    const children=E('div',undefined,'object-children');body.insertBefore(children,body.querySelector(':scope > .object-secondary'));
    if(!detail.historical){if(kids)appendObjectPage(children,kids,childParams,generation);else await appendObjectChildren(children,item.address);}
  };
  card.branchPrepare=async()=>{if(loaded)return;loaded=true;try{await fill();}catch(e){loaded=false;body.replaceChildren();notify(e);throw e;}};
  card.addEventListener('toggle',async()=>{if(!new URLSearchParams(location.search).has('object'))rememberBranch(item.address,card.open);if(card.open)syncObjectTitles(body);if(!card.open||loaded)return;loaded=true;card.branchReady=true;body.append(E('p','Loading object…','object-loading hint'));try{const before=card.getBoundingClientRect().height;await fill();stopBranchAnimation(card);animateBranchHeight(card,before,card.getBoundingClientRect().height);}catch(e){loaded=false;body.replaceChildren();notify(e);}});
  return card;
}
function syncObjectTitles(root=document){
  for(const title of root.querySelectorAll('.object-full-title')){
    const container=title.closest('.common-object,.notes-dialog');
    const heading=container?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
    title.hidden=!heading||(heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1);
  }
}
window.addEventListener('resize',()=>syncObjectTitles());
function objectSection(title,cls){
  const group=E('details',undefined,'object-section '+cls);
  group.append(E('summary',title));enableBranchMotion(group);return group;
}
// The schema selects the next types that can contain verification criteria.
function verificationChildTitle(kind){
  const types=data.objectTypes,branches=new Set(Object.keys(types).filter(k=>types[k].role==='ac'));
  let changed=true;
  while(changed){const size=branches.size;for(const child of [...branches])for(const parent of types[child]?.parents||[])if(parent==='*')Object.keys(types).forEach(k=>branches.add(k));else branches.add(parent);changed=branches.size!==size;}
  return [...new Set(Object.entries(types).filter(([k,t])=>branches.has(k)&&(t.parents||[]).some(p=>p===kind||p==='*')).map(([,t])=>t.title))].join(' / ');
}
function showTestCount(chip){
  const green=Number(chip.dataset.passed),red=Number(chip.dataset.failed),total=Number(chip.dataset.total),title=chip.dataset.childTitle;
  chip.textContent=(green?green+' / '+total:total)+' '+title;
  chip.classList.toggle('has-passes',green>0);chip.classList.toggle('has-failures',red>0);
  const label=green+' of '+total+' '+title+' ready'+(red?', '+red+' failed':'');
  chip.setAttribute('aria-label',label);
  const tone=red?'red':total>0&&green===total?'green':green>0?'yellow':'grey';
  const dot=chip.closest('summary')?.querySelector('.status-circle');
  if(dot&&(dot.dataset.tone!==tone||dot.dataset.hint!=='Status: '+label))dot.replaceWith(statusCircle({tone,label}));
}
// A criterion takes its dot from its tests: green when every test passes,
// red when one fails or is missing, grey otherwise, and grey "No tests" without tests.
const testTone=status=>status==='pass'||status==='green'?'green':['fail','missing','red'].includes(status)?'red':'grey';
function testSummary(s){
  const passed=s.tests.filter(t=>t.status==='pass').length;
  return 'Tests: '+passed+' of '+s.tests.length+' pass'+(s.commit?' · commit '+s.commit.slice(0,7):'')+(s.commits_behind?' · '+s.commits_behind+' commits behind':'')+(s.tests_draft?' · list needs saving':'');
}
async function applyTestStatuses(items){
  const criteria=items.filter(item=>data.objectTypes?.[item.kind]?.role==='ac').map(item=>item.address);
  if(!criteria.length)return;
  let reply;
  try{reply=await api('workspaces/'+encodeURIComponent(ws)+'/tests?ac='+encodeURIComponent(criteria.join(',')));}catch{return;}
  for(const status of reply.items||[reply]){
    for(const card of document.querySelectorAll('.common-object[data-object-address="'+CSS.escape(status.address)+'"]')){
      const dot=card.querySelector(':scope > summary .status-circle');
      // A criterion without tests is grey "No tests", so yellow on the board means mixed only.
      const label=status.reason==='no tests'?'No tests':testSummary(status);
      if(dot)dot.replaceWith(statusCircle({tone:testTone(status.status),label}));
    }
  }
}
// The tests that prove a criterion, each with its latest result.
function criterionTestsSection(address){
  const group=objectSection('Tests','object-tests');
  api('workspaces/'+encodeURIComponent(ws)+'/tests?ac='+encodeURIComponent(address)).then(s=>{
    group.querySelector(':scope > summary').textContent='Tests '+s.tests.length;
    if(!s.tests.length){group.append(E('p','No tests are listed for this criterion.','hint'));return;}
    group.append(E('p',testSummary(s),'node-meta'));
    if(s.tests_draft)group.append(E('p','The criterion text changed. Save the test list again.','dependency-review'));
    for(const t of s.tests){
      const row=E('div',undefined,'ac-check');
      const head=E('p',undefined,'ac-check-heading');
      head.append(statusCircle({tone:testTone(t.status),label:t.status.replace('_',' ')}),E('span',t.address));
      row.append(head,E('p',[t.status.replace('_',' '),t.commit&&'commit '+t.commit.slice(0,7)].filter(Boolean).join(' · '),'detail-fact'));
      if(t.message)row.append(E('pre',t.message));
      group.append(row);
    }
  }).catch(e=>group.append(E('p',e.message,'hint')));
  return group;
}
function fillCommonObject(body,detail){
  body.dataset.objectContextAddress=detail.address;
  const r=detail.document,d=r.data;
  const historical=detail.historical??r.revision!==detail.current_revision;
  const heading=body.parentElement?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
  const full=heading?E('div',undefined,'object-field object-full-title'):null;
  if(full){full.hidden=heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1;full.append(E('small','Full title','type-label'),E('p',d.title,'node-description'));}
  if(historical)body.append(E('p','Saved document revision. Current status, comments, files and incoming links are not shown.','hint'));
  if(r.present===false)body.append(E('p','Historical object. This object is absent from the current tree.','dependency-review'));
  const secondary=E('div',undefined,'object-secondary');
  // A view without the row Fields button, such as the revision dialog, always shows the fields.
  const fields=E('div',undefined,'object-details'),fieldsToggle=body.parentElement?.querySelector(':scope > summary .fields-button');
  fields.hidden=!!fieldsToggle&&fieldsToggle.getAttribute('aria-expanded')!=='true';
  const meta=E('div',undefined,'object-meta');
  meta.append(E('span','Revision '+r.revision),E('span','Owner: '+personLabel(d.owner||'Unassigned')));fields.append(meta);
  if(full)fields.append(full);
  const verification=detail.process?.verification&&({pass:'Passed',fail:'Failed',unverified:'Unverified'}[detail.process.verification]||detail.process.verification);
  if(verification)fields.append(E('p','Verification: '+verification,'node-meta'));
  const status=typeof detail.process?.status==='string'?detail.process.status:detail.signal?.label;
  if(detail.process?.status&&!inputQueue(detail.kind)&&status!==verification&&status!=='Checks '+verification?.toLowerCase())fields.append(E('p','Status: '+status,'node-meta'));
  const assessment=detail.process?.readiness_blocker||detail.process?.assessment;
  if(!historical&&assessment)fields.append(E('p',assessmentHint(assessment).trim(),'node-meta'));
  if(detail.review_required)body.append(E('p','A dependency changed. Review the linked revision.','dependency-review'));
  const contentKey=['body','description','contract'].find(key=>typeof d[key]==='string'&&d[key].trim());
  if(contentKey)body.append(markdownBlock(d[contentKey],'object-content node-description'));
  for(const [key,value] of Object.entries(d)){
    if(['title','owner',contentKey].includes(key)||value===null||value===''||(Array.isArray(value)&&value.length===0))continue;
    const field=E('div',undefined,'object-field');
    const refRule=data.objectTypes?.[detail.kind]?.references?.[key];
    field.append(E('small',refRule?refRule.title:key.replaceAll('_',' '),'type-label'));
    if(refRule){
      const vals=Array.isArray(value)?value:[value];
      const refs=E('div',undefined,'object-links reference-links');
      for(const v of vals){
        const targetAddress=v.includes(':')?v:refRule.target+':'+v;
        const title=detail.references?.find(l=>l.target===targetAddress)?.title||detail.links?.find(l=>l.link.target===targetAddress)?.title||v;
        refs.append(objectLink(String(title),targetAddress));
      }
      field.append(refs);
    }else{
      field.append(E('p',typeof value==='string'?value:JSON.stringify(value),'node-description'));
    }
    fields.append(field);
  }
  secondary.append(fields);
  if(!historical&&data.objectTypes?.[detail.kind]?.role==='ac')secondary.append(criterionTestsSection(detail.address));
  const links=new Map();
  for(const {link,title,current_revision,present} of detail.links||[]){const key=link.target+'@'+(link.revision||'');const label='Links to · '+link.relation+(link.revision?' · revision '+link.revision:'')+(link.revision&&current_revision!==link.revision?' · changed':'')+(present===false?' · historical object':'')+(historical?' · current title':'');const entry=links.get(key)||{address:link.target,title:title||link.target,revision:link.revision,labels:[]};entry.labels.push(label);links.set(key,entry);}
  for(const link of detail.incoming||[]){const key=link.source+'@';const entry=links.get(key)||{address:link.source,title:link.title||link.source,labels:[]};entry.labels.push('Linked from · '+link.relation);links.set(key,entry);}
  if(links.size){
    const group=objectSection('Links '+links.size,'object-relations');
    const rows=E('div',undefined,'object-links');
    for(const entry of links.values()){const row=objectLink(entry.title,entry.address,entry.revision);row.append(E('small',[...new Set(entry.labels)].join(' · '),'object-link-meta'));rows.append(row);}
    group.append(rows);secondary.append(group);
  }
  const annotations=historical?{}:detail.annotations||{};
  if(annotations.description){const context=E('div',undefined,'object-field');context.append(E('small','Context','type-label'),E('p',annotations.description,'node-description'));fields.append(context);}
  const comments=Object.values(annotations.comments||{}).filter(comment=>!comment.withdrawn);
  if(comments.length){
    const group=objectSection(comments.length+' '+(comments.length===1?'comment':'comments'),'object-comments');
    for(const comment of comments){const row=E('article',undefined,'note');row.append(E('p',personLabel(comment.actor)+' · '+when(comment.at),'note-meta'),markdownBlock(comment.text,'note-text'));group.append(row);}
    secondary.append(group);
  }
  const files=historical?[]:Array.isArray(detail.files)?detail.files.map(file=>[file.id,file]):Object.entries(annotations.attachments||{}).filter(([,file])=>!file.removed);if(files.length){const group=objectSection('Files '+files.length,'object-files');group.append(attachmentList(files));secondary.append(group);}
  body.append(secondary);
}
async function showObjectRevisions(address){
  const generation=loadNumber;
  const detail=await objectPart({address});if(generation!==loadNumber)return;
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label',detail.document.data.title);
  applyObjectColor(dialog,detail.kind);
  const header=E('div',undefined,'history-dialog-header');
  const heading=E('div'),title=E('h2');
  const setTitle=record=>{const text=record.document.data.title;title.textContent=objectTitlePreview(text,record.kind);title.dataset.truncated=String(title.textContent!==text);};
  setTitle(detail);heading.append(title);header.append(heading);
  const close=button('Close',()=>dialog.close(),'history-close');header.append(close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);fillCommonObject(body,detail);
  const revisions=E('label',undefined,'dialog-revisions');revisions.append(E('span','Object revision'));const select=E('select');select.setAttribute('aria-label','Object revision');for(let n=detail.current_revision;n>0;n--){const option=E('option','Revision '+n);option.value=n;select.append(option);}revisions.append(select);header.append(revisions);let request=0;select.onchange=async()=>{const mine=++request;try{const old=await objectPart({address,revision:select.value});if(mine!==request||!dialog.isConnected)return;setTitle(old);body.replaceChildren();fillCommonObject(body,old);}catch(e){notify(e);}};
  dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();
}
