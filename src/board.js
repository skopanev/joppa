/* SNAPSHOT */
const FILE_ICON='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 13h8 M8 17h5';
let ws = '', data = null, viewerActor = '';
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
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
  if(r.status===404 && path.startsWith('workspaces/')){data=null;$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));}
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
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)+'/board'));
  if (request !== loadNumber) return;
  data = next;
  data.objectTypes=next.object_types||{};
  $('tree-schema-link').href='/tree?workspace='+encodeURIComponent(ws);
  await renderObjectTree();
}
async function boardPart(level,id,revision){
  const params=new URLSearchParams({level});
  if(id)params.set('id',id);
  if(revision)params.set('revision',revision);
  return api('workspaces/'+encodeURIComponent(ws)+'/board?'+params);
}
async function showApp(identity) {
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
$('tree').replaceChildren();
  try{await load();}catch(e){notify(e);}
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
  if(signal?.stale_after&&Date.parse(signal.stale_after)<=Date.now()&&signal.tone!=='red')
    signal={...signal,tone:'orange',label:signal.status?'Checks stale':'Contains stale checks'};
  const circle=E('span',number===undefined?undefined:String(number),'status-circle '+signal.tone+(number===undefined?'':' numbered'));
  const hint='Status: '+signal.label+(signal.at?'\nStatus event: '+eventDate(signal.at):'');
  circle.dataset.tone=signal.tone;circle.dataset.hint=hint;circle.tabIndex=0;
  circle.setAttribute('role','img');circle.setAttribute('aria-label',hint);
  circle.addEventListener('pointerenter',()=>showStatusHint(circle));
  circle.addEventListener('pointerleave',hideStatusHint);
  circle.addEventListener('focus',()=>showStatusHint(circle));
  circle.addEventListener('blur',hideStatusHint);
  return circle;
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
  if(!nav){nav=E('div',undefined,'collection-tabs');nav.setAttribute('role','tablist');nav.setAttribute('aria-label','Root collections');root.querySelector('.tree-intro').after(nav);}
  nav.replaceChildren();nav.hidden=false;
  const key='joppa-collection:'+ws;
  let selected;
  try{selected=sessionStorage.getItem(key);}catch{}
  if(!panels.some(panel=>panel.dataset.collectionKind===selected))selected=panels[0]?.dataset.collectionKind;
  const activate=(kind,focus=false)=>{
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
function cardFact(root,label,value){
  if(value===undefined||value===null||value==='')return;
  const line=E('p',undefined,'detail-fact');
  line.append(E('span',label+': ','hint'),E('span',value));root.append(line);
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
  const color=data.objectTypes?.[kind]?.color||'#63756b';
  if(/^#[0-9a-f]{6}$/i.test(color))node.style.setProperty('--object-color',color);
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
    if(![...holder.querySelectorAll(':scope > .common-object')].some(card=>card.dataset.objectAddress===item.address))holder.append(commonObjectCard(item));
  }
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
  const intro=E('div',undefined,'tree-intro');intro.append(E('h1',data.state.title),E('p','Open an object to read its details and children.','hint'));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(data.launch.blocker_count??data.launch.blockers?.length),'hint'));
  root.append(intro);
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
function commonObjectCard(item){
  const card=E('details',undefined,'common-object');card.dataset.objectAddress=item.address;card.dataset.kind=item.kind;applyObjectColor(card,item.kind);
  const summary=E('summary',undefined,'title-row');
  const tone=item.review_required?'purple':item.work_ready?'grey':'yellow';
  const signal=item.present===false?{tone:'grey',label:'Historical object',at:item.at}:item.signal?.tone?item.signal:{tone,label:item.review_required?'Dependency changed':item.work_ready?'Current document':'Dependency not ready',at:item.at};
  summary.append(nodeTitle(item.title,signal),E('small',data.objectTypes?.[item.kind]?.title||item.kind.toUpperCase(),'type-label'));
  if(inputQueue(item.kind))summary.append(E('span',item.note_status==='processed'?'Processed':'Unprocessed','note-state'));
  const actions=E('span',undefined,'item-actions');
  const copy=copyButton(item.item_id||item.address);
  const history=button('Revisions',()=>showCommonObject(item.address,true),'history-button');history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});actions.append(copy,history);summary.append(actions);card.append(summary);
  const body=E('div',undefined,'object-body');card.append(body);enableBranchMotion(card);let loaded=false;
  card.addEventListener('toggle',async()=>{if(!card.open||loaded)return;loaded=true;const generation=loadNumber;body.append(E('p','Loading object…','object-loading hint'));try{const detail=await objectPart({address:item.address});if(generation!==loadNumber||!card.isConnected)return;resizeBranch(card,()=>{body.replaceChildren();fillCommonObject(body,detail);});const children=E('div',undefined,'object-children');body.append(children);await appendObjectChildren(children,item.address);}catch(e){loaded=false;body.replaceChildren();notify(e);}});
  return card;
}
function fillCommonObject(body,detail){
  body.dataset.objectContextAddress=detail.address;
  const r=detail.document,d=r.data;
  const historical=detail.historical??r.revision!==detail.current_revision;
  if(historical)body.append(E('p','Saved document revision. Current status, comments, files and incoming links are not shown.','hint'));
  if(r.present===false)body.append(E('p','Historical object. This object is absent from the current verification revision.','dependency-review'));
  const meta=E('div',undefined,'object-meta');
  meta.append(E('span','Revision '+r.revision),E('span','Owner: '+personLabel(d.owner||'Unassigned')));body.append(meta);
  if(detail.process?.verification&&(detail.process.req||data.objectTypes?.[detail.kind]?.role==='task'))body.append(E('p','Verification: '+({pass:'Passed',fail:'Failed',unverified:'Unverified'}[detail.process.verification]||detail.process.verification),'node-meta'));
  if(detail.process?.status&&!inputQueue(detail.kind))body.append(E('p','Status: '+(typeof detail.process.status==='string'?detail.process.status:detail.signal?.label||'Current'),'node-meta'));
  if(detail.review_required)body.append(E('p','A dependency changed. Review the linked revision.','dependency-review'));
  for(const [key,value] of Object.entries(d)){
    if(['title','owner'].includes(key)||(inputQueue(detail.kind)&&key==='status')||value===null||value===''||(Array.isArray(value)&&value.length===0))continue;
    const field=E('div',undefined,'object-field');
    const refRule=data.objectTypes?.[detail.kind]?.references?.[key];
    field.append(E('small',refRule?refRule.title:key.replaceAll('_',' '),'type-label'));
    if(refRule){
      const vals=Array.isArray(value)?value:[value];
      const refs=E('div',undefined,'object-links reference-links');
      for(const v of vals){
        const targetAddress=v.includes(':')?v:refRule.target+':'+v;
        const title=detail.references?.find(l=>l.target===targetAddress)?.title||detail.links?.find(l=>l.link.target===targetAddress)?.title||v;
        refs.append(button(String(title),()=>showCommonObject(targetAddress),'object-link'));
      }
      field.append(refs);
    }else{
      field.append(E('p',typeof value==='string'?value:JSON.stringify(value),'node-description'));
    }
    body.append(field);
  }
  if(detail.links?.length){const group=E('div',undefined,'object-links');group.append(E('h3','Links'));for(const {link,title,current_revision,present} of detail.links){const row=button((title||link.target)+' · '+link.relation+(link.revision?' · revision '+link.revision:'')+(link.revision&&current_revision!==link.revision?' · changed':'')+(present===false?' · historical object':'')+(historical?' · current title':''),()=>showCommonObject(link.target,false,link.revision),'object-link');group.append(row);}body.append(group);}
  if(detail.incoming?.length){const group=E('div',undefined,'object-links');group.append(E('h3','Linked from'));for(const link of detail.incoming)group.append(button(link.title+' · '+link.relation,()=>showCommonObject(link.source),'object-link'));body.append(group);}
  const annotations=historical?{}:detail.annotations||{};
  if(annotations.description){const context=E('div',undefined,'object-field');context.append(E('small','Context','type-label'),E('p',annotations.description,'node-description'));body.append(context);}
  for(const comment of Object.values(annotations.comments||{})){if(comment.withdrawn)continue;const row=E('article',undefined,'note');row.append(E('p',personLabel(comment.actor)+' · '+when(comment.at),'note-meta'),E('p',comment.text,'note-text'));body.append(row);}
  const files=Object.entries(annotations.attachments||{}).filter(([,file])=>!file.removed);if(files.length)body.append(attachmentList(files));
  if(!historical)for(const kind of inboxKinds())commonNotes(body,detail.address,kind);
  if(!historical&&detail.process?.req){
    const inspect=button('Checks',async()=>{
      inspect.disabled=true;
      try{const record=window.SNAPSHOT?snapshotVerification(detail):await boardPart('process',detail.address);const content=E('section',undefined,'verification-record');renderVerification(content,record);body.append(content);inspect.remove();}
      finally{inspect.disabled=false;}
    },'text-button');body.append(inspect);
  }
}
function snapshotVerification(detail){
  const req=detail.process.req,r=data.state.requirements[req].revisions.at(-1);
  const criteria=Object.values(data.snapshot_objects).filter(o=>o.process?.req===req&&data.objectTypes[o.kind]?.role==='ac'&&o.revisions.at(-1).present).map(o=>({address:o.address,id:o.process.criterion||o.id,title:o.revisions.at(-1).data.title}));
  return {...r,revision:r.number,criteria};
}
function renderVerification(root,record){
  root.append(E('h3','Checks'),E('p','Revision '+record.revision+(record.confirmed?' · Confirmed':' · Awaiting owner confirmation'),'node-meta'));
  if(record.verification_scope)root.append(E('p',record.verification_scope.scope,'detail-fact'));
  for(const criterion of record.criteria||[]){
    const item=E('section',undefined,'ac-checks');
    const heading=E('h4',undefined,'ac-check-heading');
    heading.append(statusCircle(criterion.signal||{tone:'grey',label:'Not checked'}),button(criterion.title,()=>showCommonObject(criterion.address),'object-link'));item.append(heading);
    const runs=Object.entries(record.runs||{});
    let count=0;
    for(const [id,run] of runs){
      for(const check of run.plan||[]){
        if(check.ac!==criterion.id)continue;
        count++;const result=run.results?.[check.id];
        const row=E('article',undefined,'ac-check');
        const at=record.result_dates?.[id]?.[check.id];
        const stale=result?.outcome==='pass'&&at&&Date.now()-Date.parse(at)>7*24*60*60*1000;
        const label=stale?'Passed · older than 7 days':result?.outcome==='pass'?'Passed':result?.outcome==='fail'?'Failed':result?.outcome==='skip'?'Skipped':result?'Not checked':run.finished?'Missing result':'Running';
        const name=E('h4',undefined,'ac-check-heading');name.append(statusCircle({tone:result?.outcome==='fail'?'red':stale?'orange':result?.outcome==='pass'?'green':result?'grey':'yellow',label,at}),E('span',check.id));
        row.append(name,E('p',label+' · '+run.scope,'node-meta'));
        cardFact(row,'Expected',(check.expected||[]).join(', '));
        if(result){cardFact(row,'Observed',(result.observed||[]).join(', ')||'Nothing recorded');if(result.note)cardFact(row,'Evidence',result.note);}
        item.append(row);
      }
    }
    if(!count)item.append(E('p','No tests linked yet.','hint'));
    root.append(item);
  }
}
async function showCommonObject(address,history=false,revision){
  const generation=loadNumber;
  const detail=await objectPart(revision?{address,revision}:{address});if(generation!==loadNumber)return;
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label',detail.document.data.title);
  const header=E('div',undefined,'history-dialog-header');
  const heading=E('div');heading.append(E('h2',detail.document.data.title));header.append(heading);
  const close=button('Close',()=>dialog.close(),'history-close');header.append(close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);fillCommonObject(body,detail);
  if(history){const revisions=E('label',undefined,'dialog-revisions');revisions.append(E('span','Object revision'));const select=E('select');select.setAttribute('aria-label','Object revision');for(let n=detail.current_revision;n>0;n--){const option=E('option','Revision '+n);option.value=n;select.append(option);}revisions.append(select);header.append(revisions);let request=0;select.onchange=async()=>{const mine=++request;try{const old=await objectPart({address,revision:select.value});if(mine!==request||!dialog.isConnected)return;body.replaceChildren();fillCommonObject(body,old);}catch(e){notify(e);}};}
  dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();
}
