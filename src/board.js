/* SNAPSHOT */
const FILE_ICON='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 13h8 M8 17h5';
let ws = '', data = null, viewerActor = '';
let filterMenu=null;
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
let noticeTimer;
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
  toastTimer=setTimeout(()=>{host.classList.remove('toast-visible');host.classList.add('toast-leaving');},3000);
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
  if(!copied){toast('Could not copy the ID. Select it by hand.',true);return false;}
  const label=trigger.dataset.label||trigger.textContent;
  trigger.dataset.label=label;trigger.textContent='Copied';
  setTimeout(()=>{if(trigger.isConnected)trigger.textContent=label;},3000);
  toast('Copied '+text);
  return true;
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
  b.onclick = () => Promise.resolve().then(fn).catch(notify);
  return b;
}
function details(parent, title) {
  const d = E('details');
  d.append(E('summary', title));
  parent.append(d);
  return d;
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
const loadedDomains=new Set(),loadedCapabilities=new Set(),loadedRequirements=new Set();
let filterLoaded=false,filterLoading=null;
const fullHistoryRequirements=new Set();
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
  loadedDomains.clear();loadedCapabilities.clear();loadedRequirements.clear();
  fullHistoryRequirements.clear();
  filterLoaded=false;filterLoading=null;
  await renderObjectTree();
}
async function boardPart(level,id,revision){
  const params=new URLSearchParams({level});
  if(id)params.set('id',id);
  if(revision)params.set('revision',revision);
  return api('workspaces/'+encodeURIComponent(ws)+'/board?'+params);
}
async function ensureFilterIndex(){
  if(!data?.lazy||filterLoaded)return;
  if(filterLoading)return filterLoading;
  const request=loadNumber;
  filterLoading=(async()=>{
    const part=await boardPart('filter');
    if(request!==loadNumber)return;
    Object.assign(data.state.capabilities,part.capabilities);
    for(const [id,q] of Object.entries(part.requirements))if(!loadedRequirements.has(id)&&!fullHistoryRequirements.has(id))data.state.requirements[id]=q;
    Object.assign(data.state.tree_order,part.tree_order);
    Object.assign(data.summary,part.summary);
    Object.assign(data.signals.capabilities,part.signals.capabilities);
    Object.assign(data.signals.requirements,part.signals.requirements);
    Object.assign(data.system_ids.capability,part.system_ids.capability);
    Object.assign(data.system_ids.requirement,part.system_ids.requirement);
    Object.keys(data.state.domains).forEach(id=>loadedDomains.add(id));
    Object.keys(data.state.capabilities).forEach(id=>loadedCapabilities.add(id));
    filterLoaded=true;render();applyFilters();
  })();
  try{await filterLoading;}finally{filterLoading=null;}
}
async function showApp(identity) {
  if(identity.email===undefined)identity={...identity,...await api('me')};
  viewerActor=identity.actor;
  $('identity').textContent=identity.email||'Account';
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
const statusLabels={waiting:'Waiting for decision',review:'Task review required',available:'Ready to work',progress:'In progress',ready:'Awaiting acceptance',pass:'All ACs green',fail:'Checks failed'};
let filters={domain:'',status:'',person:''};
let filterWorkspace=null,statusCounts={};
function requirementStatus(id) {
  if(data.lazy)return data.signals.requirements[id]?.status||'waiting';
  const r=data.state.requirements[id].revisions.at(-1),s=data.summary[id];
  if(Object.values(s.acs).some(a=>a.verification==='fail'))return 'fail';
  if(s.pending_run)return 'progress';
  if(s.ready_for_acceptance)return 'pass';
  if(!r.confirmed||!r.acs.length||Object.values(s.acs).some(a=>!a.ready))return 'waiting';
  if(Object.values(r.tasks).some(t=>t.review_required))return 'review';
  if(Object.keys(r.runs).length||Object.values(r.tasks).some(t=>t.assignee&&t.done===null&&!t.retired))return 'progress';
  return 'available';
}
function requirementSignal(id){
  if(data.lazy)return data.signals.requirements[id]||{tone:'grey',label:'No status yet'};
  const r=data.state.requirements[id].revisions.at(-1),s=data.summary[id];
  const at=statusEventDate(id,r.number);
  if(!r.acs.length)return {tone:'grey',label:'No acceptance criteria',at};
  const acSignals=r.acs.map(a=>acSignal(s.acs[a.id],r,a.id,id));
  if(acSignals.some(signal=>signal.tone==='red'))return {tone:'red',label:s.pending_run?'Checks failed · newer run in progress':'Checks failed',at};
  if(s.pending_run)return {tone:'yellow',label:'Newer checks running · previous result is historical',at};
  if(acSignals.some(signal=>signal.tone==='orange'))return {tone:'orange',label:'Checks stale',at};
  const status=requirementStatus(id);
  return {tone:status==='fail'?'red':status==='pass'?'green':status==='review'?'purple':status==='available'?'grey':'yellow',label:statusLabels[status],at};
}
function aggregateSignal(children,noun){
  if(!children.length)return {tone:'grey',label:'No '+noun};
  const counts={green:0,yellow:0,orange:0,red:0,purple:0,grey:0};
  for(const child of children)counts[child.tone]++;
  const tone=counts.red?'red':counts.orange?'orange':counts.purple?'purple':counts.yellow?'yellow':counts.grey?'grey':'green';
  const parts=[[counts.green,'complete'],[counts.orange,'stale'],[counts.purple,'needs review'],[counts.yellow,'pending'],[counts.red,'failed'],[counts.grey,'unchecked or empty']].filter(([n])=>n).map(([n,label])=>n+' '+label);
  return {tone,label:'Current '+noun+': '+parts.join(' · '),at:children.map(child=>child.at).filter(Boolean).sort().at(-1)};
}
function treeSignals(){
  if(data.lazy)return data.signals;
  const requirements=Object.fromEntries(Object.keys(data.state.requirements).map(id=>[id,requirementSignal(id)]));
  const capabilities=Object.fromEntries(Object.keys(data.state.capabilities).map(id=>[id,aggregateSignal(Object.entries(data.state.requirements).filter(([,q])=>q.capability===id).map(([rid])=>requirements[rid]),'requirements')]));
  const domains=Object.fromEntries(Object.keys(data.state.domains).map(id=>[id,aggregateSignal(Object.entries(data.state.capabilities).filter(([,c])=>c.domain===id).map(([cid])=>capabilities[cid]),'capabilities')]));
  return {requirements,capabilities,domains};
}
function latestCheckRun(r){
  return Object.entries(r.runs||{}).filter(([,run])=>run.finished!==null&&run.finished!==undefined).sort(([,a],[,b])=>(b.started||0)-(a.started||0))[0];
}
function checkRecordedAt(r,entry,ac,reqId){
  const dates=[];
  const [runId,run]=entry||[];
  for(const check of run?.plan||[])if(check.ac===ac&&run.results?.[check.id]){
    const event=resultEvent(reqId,r.number,runId,check.id);
    if(event?.recorded_at)dates.push(event.recorded_at);
  }
  return dates;
}
function staleCheck(recordedAt){
  const time=Date.parse(String(recordedAt||'').replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isFinite(time)&&Date.now()-time>7*24*60*60*1000;
}
let statusEventIndex=new Map();
let resultEventIndex=new Map();
function resultEvent(reqId,revision,runId,checkId){
  return resultEventIndex.get(JSON.stringify([reqId,revision,runId,checkId]));
}
function indexStatusEvents(){
  statusEventIndex=new Map();
  resultEventIndex=new Map();
  const revisions=new Map();
  for(const [req,q] of Object.entries(data.state.requirements))for(const r of q.revisions)revisions.set(req+'/'+r.number,r);
  const relevant=new Set(['assess','confirm','task','amend_task','claim_task','release_task','review_task','retire_task','complete_task','define_verification_scope','plan','review_coverage','result','finish','issue','resolve','withdraw_decision','accept','revoke']);
  for(const event of data.history||[]){
    const c=event.command;
    if(c.op==='result')resultEventIndex.set(JSON.stringify([c.req,c.revision,c.run,c.evidence.check]),event);
    if(!c.req||c.op!=='revise'&&!relevant.has(c.op))continue;
    const revision=c.op==='revise'?c.expected_revision+1:c.revision;
    if(!revision)continue;
    const key=c.req+'/'+revision;
    statusEventIndex.set(key,event.recorded_at);
    const r=revisions.get(key);
    let acs=[];
    if(c.op==='revise')acs=c.acs.map(a=>a.id);
    else if(c.ac)acs=[c.ac];
    else if(c.acs)acs=c.acs;
    else if(['amend_task','claim_task','release_task','complete_task'].includes(c.op))acs=r?.tasks?.[c.id]?.acs||[];
    else if(c.op==='retire_task')acs=r?.tasks?.[c.id]?.acs||[];
    else if(c.op==='plan')acs=c.checks.map(check=>check.ac);
    else if(c.op==='result')acs=r?.runs?.[c.run]?.plan.filter(check=>check.id===c.evidence.check).map(check=>check.ac)||[];
    else if(c.op==='finish')acs=r?.runs?.[c.run]?.plan.map(check=>check.ac)||[];
    else if(c.op==='resolve')acs=[r?.issues?.[c.id]?.ac].filter(Boolean);
    else if(c.op==='withdraw_decision')acs=[data.state.requirements[c.req]?.comments?.[c.comment]?.ac].filter(Boolean);
    for(const ac of acs)statusEventIndex.set(key+'/'+ac,event.recorded_at);
  }
}
function statusEventDate(reqId,revision,acId){
  return statusEventIndex.get(reqId+'/'+revision+(acId?'/'+acId:''));
}
function acSignal(status,r,id,reqId){
  const at=statusEventDate(reqId,r.number,id);
  if(!status)return {tone:'grey',label:'Historical revision',at};
  if(status.verification==='fail')return {tone:'red',label:data.summary[reqId]?.pending_run?'Checks failed · newer run in progress':'Checks failed',at};
  if(data.summary[reqId]?.pending_run)return {tone:'yellow',label:'Newer checks running · previous result is historical',at};
  if(status.verification==='pass'){
    const run=latestCheckRun(r),stale=(checkRecordedAt(r,run,id,reqId)||[]).some(staleCheck);
    return stale?{tone:'orange',label:'Checks passed · older than 7 days',at}:{tone:'green',label:'Checks passed',at};
  }
  if(!status.ready)return {tone:'yellow',label:'Waiting for decision',at};
  if(Object.values(r.tasks).some(t=>t.acs.includes(id)&&t.review_required))return {tone:'purple',label:'Task review required',at};
  const working=Object.values(r.tasks).some(t=>!t.retired&&t.assignee&&t.acs.includes(id)&&t.done===null)||Object.values(r.runs).some(run=>!run.finished&&run.plan.some(check=>check.ac===id));
  return {tone:working?'yellow':'grey',label:working?'In progress':'Ready to work · Not checked',at};
}
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
function matchesPerson(id) {
  const actor=filters.person==='@me'?viewerActor:filters.person;
  if(!actor)return true;
  if(data.lazy&&!filterLoaded&&!loadedRequirements.has(id))return true;
  const r=data.state.requirements[id].revisions.at(-1);
  return r.owner===actor||Object.values(r.tasks).some(t=>!t.done&&t.assignee===actor)||
    r.acs.some(a=>!data.summary[id].acs[a.id]?.ready&&(a.owner===actor||a.assessment?.owner===actor));
}
function closeFilterMenu(restore=false){
  if(!filterMenu)return;
  const {menu,trigger}=filterMenu;menu.remove();trigger.setAttribute('aria-expanded','false');filterMenu=null;
  if(restore)trigger.focus();
}
document.addEventListener('click',e=>{if(filterMenu&&!filterMenu.host.contains(e.target))closeFilterMenu();});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&filterMenu){e.preventDefault();closeFilterMenu(true);}});
function filterPicker(kind,label,caption,options,value,change){
  const host=E('div',undefined,'filter-picker '+kind+'-picker');
  const trigger=E('button',undefined,'filter-select');trigger.type='button';trigger.id=kind+'-filter';
  for(const [key,val] of Object.entries({role:'combobox','aria-label':label,'aria-haspopup':'listbox','aria-expanded':'false','aria-controls':kind+'-options'}))trigger.setAttribute(key,val);
  const mark=E('span',kind==='domain'?'◈':kind==='person'?'◎':'●','picker-mark');mark.setAttribute('aria-hidden','true');
  if(kind==='status')mark.className='picker-mark '+value;
  const text=E('span',undefined,'picker-text'),selected=E('span',options.find(o=>o.id===value)?.title,'picker-value');
  text.append(selected);
  const captionLabel=E('label',caption,'filter-caption');captionLabel.htmlFor=trigger.id;
  const chevron=E('span','⌄','picker-chevron');chevron.setAttribute('aria-hidden','true');trigger.append(mark,text,chevron);host.append(captionLabel,trigger);
  function open(last=false){
    closeFilterMenu();
    const menu=E('div',undefined,'filter-menu');menu.id=kind+'-options';menu.setAttribute('role','listbox');menu.setAttribute('aria-label',label);
    for(const option of options){
      const row=E('button',undefined,'filter-option'+(kind==='status'?' status-option '+(option.id||'all'):''));row.type='button';row.tabIndex=-1;row.setAttribute('role','option');row.setAttribute('aria-label',option.title);row.setAttribute('aria-selected',String(option.id===value));
      row.append(E('span',option.title),E('small',typeof option.hint==='function'?option.hint():option.hint),E('span',option.id===value?'✓':'','option-check'));
      row.onclick=()=>{value=option.id;selected.textContent=option.title;if(kind==='status')mark.className='picker-mark '+value;closeFilterMenu(true);change(value);};menu.append(row);
    }
    menu.onkeydown=e=>{
      const rows=[...menu.children],index=rows.indexOf(document.activeElement);let next;
      if(e.key==='ArrowDown')next=(index+1)%rows.length;
      if(e.key==='ArrowUp')next=(index+rows.length-1)%rows.length;
      if(e.key==='Home')next=0;if(e.key==='End')next=rows.length-1;
      if(next!==undefined){e.preventDefault();rows[next].focus();}
      if(e.key==='Tab')closeFilterMenu();
    };
    host.append(menu);filterMenu={host,menu,trigger};trigger.setAttribute('aria-expanded','true');
    (last?menu.lastElementChild:menu.querySelector('[aria-selected=true]')||menu.firstElementChild).focus();
  }
  trigger.onclick=()=>{if(filterMenu?.trigger===trigger)closeFilterMenu();else open();};
  trigger.onkeydown=e=>{if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();open(e.key==='ArrowUp');}};
  return host;
}
function filterControls(root) {
  if(filterWorkspace!==ws){
    filterWorkspace=ws;filters={domain:'',status:'',person:''};
    try{const saved=JSON.parse(sessionStorage.getItem('joppa-filters:'+ws));
      if(saved && typeof saved.domain==='string' && typeof saved.status==='string')filters={domain:saved.domain,status:saved.status,person:typeof saved.person==='string'?saved.person:''};
    }catch{}
  }
  if(filters.domain&&(!data.state.domains[filters.domain]||data.state.domains[filters.domain].archived))filters.domain='';
  if(filters.status&&!statusLabels[filters.status])filters.status='';
  const toolbar=E('section',undefined,'tree-toolbar');toolbar.setAttribute('aria-label','Filter product tree');
  const fields=E('div',undefined,'filter-fields');
  if(filters.person&&filters.person===viewerActor)filters.person='@me';
  if(filters.person&&filters.person!=='@me'&&data.state.members[filters.person]!=='human')filters.person='';
  if(filters.person==='@me'&&!viewerActor)filters.person='';
  fields.append(filterPicker('domain','Filter by domain','Domain',[
    {id:'',title:'All domains',hint:String(orderedEntries('domain').length)},
    ...orderedEntries('domain').map(([id,d])=>({id,title:d.title,hint:data.lazy&&!loadedDomains.has(id)?'Open to load':String(Object.values(data.state.capabilities).filter(c=>c.domain===id).length)+' capabilities'}))
  ],filters.domain,value=>{filters.domain=value;applyFilters();}));
  const people=[{id:'',title:'Everyone',hint:'All responsibilities'}];
  if(viewerActor)people.push({id:'@me',title:'Assigned to me',hint:'My responsibilities'});
  for(const [id,role] of Object.entries(data.state.members))if(role==='human'&&id!==viewerActor)people.push({id,title:personLabel(id),hint:'Responsibilities'});
  fields.append(filterPicker('person','Filter by person','Assigned to',people,filters.person,value=>{filters.person=value;if(value&&data.lazy&&!filterLoaded)ensureFilterIndex().catch(notify);else applyFilters();}));
  fields.prepend(fields.lastElementChild);
  toolbar.append(fields);
  fields.append(filterPicker('status','Filter by status','Status',[
    {id:'',title:'All statuses',hint:()=>Object.values(statusCounts).reduce((a,b)=>a+b,0)+' requirements'},
    ...Object.entries(statusLabels).map(([id,title])=>({id,title,hint:()=>(statusCounts[id]||0)+' requirements'}))
  ],filters.status,value=>{filters.status=value;if(value&&data.lazy&&!filterLoaded)ensureFilterIndex().catch(notify);else applyFilters();}));
  const bottom=E('div',undefined,'filter-bottom');
  const results=E('span','','filter-results');results.id='filter-results';results.setAttribute('role','status');
  bottom.append(results);toolbar.append(bottom);root.append(toolbar);
}
function applyFilters(reveal=true) {
  if(!data)return;
  try{sessionStorage.setItem('joppa-filters:'+ws,JSON.stringify(filters));}catch{}
  const counts=Object.fromEntries(Object.keys(statusLabels).map(key=>[key,0]));
  let shown=0,visibleDomains=0;
  for(const group of $('tree').querySelectorAll('.tree-domain')){
    const did=group.dataset.domain;
    const domainAllowed=!filters.domain||filters.domain===did;
    let domainVisible=false;
    for(const cap of group.querySelectorAll('.tree-capability')){
      let capVisible=false;
      for(const row of cap.querySelectorAll('.tree-requirement')){
        const id=row.dataset.requirement;
        const match=domainAllowed&&matchesPerson(id);
        const status=requirementStatus(id);
        if(match)counts[status]++;
        row.hidden=!match||!!(filters.status&&filters.status!==status);
        if(!row.hidden){shown++;capVisible=true;if(reveal&&(filters.status||filters.domain||filters.person))row.open=true;}
      }
      if(!filters.status&&!filters.person&&domainAllowed)capVisible=true;
      cap.hidden=!capVisible;domainVisible ||= capVisible;
      if(capVisible&&reveal&&(filters.status||filters.domain||filters.person))cap.open=true;
    }
    if(!filters.status&&!filters.person&&domainAllowed)domainVisible=true;
    group.hidden=!domainVisible;
    if(domainVisible){visibleDomains++;if(reveal&&(filters.status||filters.domain||filters.person))group.open=true;}
  }
  statusCounts=counts;
  $('filter-results').textContent=data.lazy&&!Object.keys(data.state.requirements).length
    ?visibleDomains+' domains · open one to load its capabilities'
    :shown+' of '+Object.keys(data.state.requirements).length+' requirements · '+visibleDomains+' domains';
  $('filter-empty').hidden=visibleDomains>0||!orderedEntries('domain').length;
}
function render() {
  hideStatusHint();
  closeFilterMenu();
  const signals=treeSignals();
  const t = $('tree');
  const opened=new Set([...t.querySelectorAll('details[data-tree-node][open]')].map(d=>d.dataset.treeNode));
  t.replaceChildren();
  const intro=E('div',undefined,'tree-intro');
  intro.append(E('h1',data.state.title));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(data.launch.blocker_count??data.launch.blockers.length),'hint'));
  t.append(intro);
  const domains=E('section',undefined,'root-collection');domains.dataset.collectionKind='domain';t.append(domains);
  filterControls(domains);
  for (const [did,d] of orderedEntries('domain'))domains.append(buildDomain(did,d,signals,opened));
  if(!orderedEntries('domain').length)domains.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
  const empty=E('p','No requirements match these filters.','filter-empty');empty.id='filter-empty';empty.hidden=true;domains.append(empty);
  applyFilters(false);
  consumers(t);
  commonNotes(t);
  rootCollectionTabs(t);
}
function rootCollectionTabs(root){
  if(!Object.keys(data.objectTypes||{}).length)return;
  const types=Object.entries(data.objectTypes||{}).filter(([,type])=>type.root)
    .sort(([a,left],[b,right])=>(left.index??Infinity)-(right.index??Infinity)||a.localeCompare(b));
  const existing=[...root.querySelectorAll('.root-collection')];
  for(const panel of existing)panel.hidden=true;
  if(!types.length){root.querySelector(':scope > .collection-tabs')?.remove();return;}
  const panels=types.map(([kind])=>{
    let panel=existing.find(node=>node.dataset.collectionKind===kind);
    if(!panel){
      panel=E('section',undefined,'root-collection');panel.dataset.collectionKind=kind;panel.dataset.collectionLazy='true';root.append(panel);
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
      const active=panel.dataset.collectionKind===kind;panel.hidden=!active;
      const tab=nav.querySelector('[aria-controls="'+panel.id+'"]');tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;
      if(active){
        if(focus)tab.focus();
        panel.collectionLoad?.();
        if(panel.matches('details')&&!panel.open&&!panel.classList.contains('empty-notes'))panel.querySelector(':scope > summary').click();
        if(panel.dataset.collectionLazy==='true'){
          delete panel.dataset.collectionLazy;
          const contents=panel.querySelector(':scope > .collection-items')||panel;
          contents.append(E('p','Loading objects…','object-loading hint'));
          appendObjectChildren(contents,undefined,kind).catch(error=>{panel.dataset.collectionLazy='true';contents.replaceChildren();notify(error);});
        }
      }
    }
  };
  for(const panel of panels){
    const kind=panel.dataset.collectionKind,type=data.objectTypes[kind];
    panel.id='collection-'+kind;panel.setAttribute('role','tabpanel');panel.setAttribute('aria-labelledby','tab-'+kind);panel.tabIndex=0;
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
function buildDomain(did,d,signals,opened){
  const group=E('details',undefined,'tree-domain');applyObjectColor(group,'domain');group.dataset.domain=did;group.dataset.treeNode='domain:'+did;group.open=opened.has(group.dataset.treeNode);
  if(data.lazy&&!loadedDomains.has(did))group.dataset.pending='true';
  const heading=E('summary',undefined,'title-row');
  heading.append(nodeTitle(d.title,signals.domains[did]),E('small',data.objectTypes?.domain?.title||'DOMAIN','type-label'),itemActions('domain',did,d.title));group.append(heading);
  enableBranchMotion(group);
  const body=E('div',undefined,'domain-body');group.append(body);
  fillDomain(body,did,d,signals,opened);
  if(data.lazy)group.addEventListener('toggle',async()=>{
    if(!group.open||loadedDomains.has(did))return;
    const request=loadNumber;
    try{
      const part=await boardPart('domain',did);
      if(request!==loadNumber||!group.isConnected)return;
      Object.assign(data.state.capabilities,part.capabilities);
      data.state.tree_order['capability:'+did]=part.order;
      Object.assign(data.signals.capabilities,part.signals);
      Object.assign(data.system_ids.capability,part.system_ids);
      loadedDomains.add(did);
      delete group.dataset.pending;
      resizeBranch(group,()=>{fillDomain(body,did,d,treeSignals(),new Set());applyFilters(false);});
    }catch(e){notify(e);}
  });
  return group;
}
function fillDomain(body,did,d,signals,opened){
  body.replaceChildren(E('p','Owner: '+personLabel(d.owner),'node-meta'));
  if(d.description?.trim())body.append(E('p',d.description,'node-description'));
  for(const [cid,c] of orderedEntries('capability',did))body.append(buildCapability(cid,c,signals,opened));
  commonNotes(body,'domain:'+did);
}
function buildCapability(cid,c,signals,opened){
  const cap=E('details',undefined,'tree-capability');applyObjectColor(cap,'capability');cap.dataset.capability=cid;cap.dataset.treeNode='capability:'+cid;cap.open=opened.has(cap.dataset.treeNode);
  if(data.lazy&&!loadedCapabilities.has(cid))cap.dataset.pending='true';
  const head=E('summary',undefined,'title-row');head.append(nodeTitle(c.title,signals.capabilities[cid]),E('small',data.objectTypes?.capability?.title||'CAPABILITY','type-label'),itemActions('capability',cid,c.title));cap.append(head);
  enableBranchMotion(cap);
  if(c.owner)cap.append(E('p','Owner: '+personLabel(c.owner),'node-meta'));
  if(c.description?.trim())cap.append(E('p',c.description,'node-description'));
  fillCapability(cap,cid,signals,opened);
  commonNotes(cap,'capability:'+cid);
  if(data.lazy)cap.addEventListener('toggle',async()=>{
    if(!cap.open||loadedCapabilities.has(cid))return;
    const request=loadNumber;
    try{
      const part=await boardPart('capability',cid);
      if(request!==loadNumber||!cap.isConnected)return;
      Object.assign(data.state.requirements,part.requirements);
      data.state.tree_order['requirement:'+cid]=part.order;
      Object.assign(data.signals.requirements,part.signals);
      Object.assign(data.system_ids.requirement,part.system_ids);
      loadedCapabilities.add(cid);
      delete cap.dataset.pending;
      resizeBranch(cap,()=>{fillCapability(cap,cid,treeSignals(),new Set());applyFilters(false);});
    }catch(e){notify(e);}
  });
  return cap;
}
function fillCapability(cap,cid,signals,opened){
  cap.querySelectorAll(':scope > .tree-requirement, :scope > .empty-state').forEach(node=>node.remove());
  const reqs=orderedEntries('requirement',cid);
  if(!reqs.length&&(!data.lazy||loadedCapabilities.has(cid)))cap.append(E('p','No requirements yet.','empty-state'));
  for(const [id,q] of reqs)cap.append(buildRequirement(id,q,signals,opened));
}
function buildRequirement(id,q,signals,opened){
  const r=q.revisions.at(-1),tone=requirementStatus(id);
  const req=E('details',undefined,'tree-requirement '+tone);applyObjectColor(req,'requirement');req.dataset.requirement=id;req.dataset.treeNode='requirement:'+id;req.open=opened.has(req.dataset.treeNode);
  if(data.lazy&&!loadedRequirements.has(id))req.dataset.pending='true';
  const reqHead=E('summary',undefined,'title-row');reqHead.append(nodeTitle(r.title,signals.requirements[id]),E('small',data.objectTypes?.requirement?.title||'REQ','type-label'),itemActions('requirement',id,r.title,null,r.number));
  const preview=E('span',r.body||'','requirement-preview');preview.hidden=!r.body?.trim();reqHead.append(preview);
  const content=E('div',undefined,'requirement-detail');req.append(reqHead,content);
  enableBranchMotion(req);
  req.addEventListener('toggle',async()=>{
    if(!req.open||content.childNodes.length)return;
    if(data.lazy&&!loadedRequirements.has(id)){
      content.append(E('p','Loading requirement…','hint'));
      const request=loadNumber;
      try{
        const part=await boardPart('requirement',id);
        if(request!==loadNumber||!req.isConnected)return;
        const old=data.state.requirements[id];
        data.state.requirements[id]=fullHistoryRequirements.has(id)&&old?.revisions.length>1
          ?{...old,...part.requirement,revisions:old.revisions.map(r=>r.number===part.requirement.revisions[0].number?part.requirement.revisions[0]:r)}
          :part.requirement;
        data.summary[id]=part.summary;
        Object.assign(data.state.notes,part.notes);
        Object.assign(data.state.consumers,part.consumers);
        Object.assign(data.state.projects,part.projects);
        Object.assign(data.system_ids.ac,part.system_ids?.ac||{});
        data.history.push(...part.status_events);
        indexStatusEvents();
        loadedRequirements.add(id);
        delete req.dataset.pending;
        resizeBranch(req,()=>requirementDetails(content,id));
      }catch(e){content.replaceChildren();notify(e);}
    }else requirementDetails(content,id);
  });
  if(req.open&&(!data.lazy||loadedRequirements.has(id)))requirementDetails(content,id);
  return req;
}
function collectionHeading(section,title,description,label){
  const summary=section.querySelector(':scope > summary,:scope > header');
  const caption=E('span',undefined,'collection-caption');
  caption.append(E('span',title,'collection-title'),E('span',description,'collection-description'));
  summary.replaceChildren(caption);
  if(label)summary.append(E('span',label,'collection-label'));
}
function consumers(root){
  const entries=Object.entries(data.state.consumers||{});
  const count=data.lazy?(data.counts?.consumers??entries.length):entries.length;
  const title=data.objectTypes?.consumer?.title||'Consumer';
  const section=E('section');section.append(E('header',undefined,'collection-heading'));root.append(section);
  collectionHeading(section,(title==='Consumer'?'Consumers':title)+' · '+count,'People and roles that use the product.','Audience');
  section.classList.add('consumer-section','root-collection');section.dataset.collectionKind='consumer';applyObjectColor(section,'consumer');
  const contents=E('div',undefined,'collection-items');section.append(contents);
  if(data.lazy){section.dataset.collectionLazy='true';return;}
  for(const [id,consumer] of Object.entries(data.state.consumers||{})){
    contents.append(commonObjectCard({address:'consumer:'+id,kind:'consumer',title:consumer.title,
      item_id:data.system_ids.consumer?.[id],work_ready:true}));
  }
}
function requirementDetails(root, selected, revisionNumber=null) {
  hideStatusHint();
  root.replaceChildren();
  const q = data.state.requirements[selected],
    r = q.revisions.find(r => r.number === revisionNumber) || q.revisions.at(-1),
    current = r === q.revisions.at(-1),
    s = current ? data.summary[selected] : null;
  root.dataset.revision=r.number;
  if(current)root.dataset.status=requirementStatus(selected);
  else delete root.dataset.status;
  const currentCircle=root.parentElement?.querySelector(':scope > summary > .node-title > .status-circle');
  if(currentCircle)currentCircle.replaceWith(statusCircle(current?requirementSignal(selected):{tone:'grey',label:'Historical revision · '+r.number,at:statusEventDate(selected,r.number)}));
  if(!current)root.append(E('h3',r.title));
  // Метка живёт в заголовке, рядом с историей, и обновляется при листании
  // ревизий: иначе она показывала бы последнюю, пока на экране предыдущая.
  const chip=root.parentElement?.querySelector('.rev-chip');
  if(chip){
    chip.textContent='rev. '+r.number;
    chip.classList.toggle('past',!current);
    chip.title=current?'Current revision':'Previous revision';
  }
  const preview=root.parentElement?.querySelector(':scope > summary > .requirement-preview');
  if(preview){preview.textContent=r.body||'';preview.hidden=!r.body?.trim();}
  if(r.verification_scope){
    cardFact(root,'Verification scope',r.verification_scope.scope);
    cardFact(root,'Required subject components',r.verification_scope.subject_keys.join(', '));
  }
  if(current&&s?.coverage_change?.length){
    const review=E('section',undefined,'coverage-change');
    review.append(E('h3','Verification coverage changed'));
    review.append(E('p',s.coverage_reviewed?'The REQ owner reviewed these removed or changed checks.':'The REQ owner must review these removed or changed checks before acceptance.'));
    const list=E('ul');
    for(const check of s.coverage_change)list.append(E('li',(check.id||'Unnamed check')+' · '+(check.expected||[]).join(', ')));
    review.append(list);root.append(review);
  }
  if(current)commonNotes(root,'requirement:'+selected);
  cardFact(root,'Consumers',(r.consumers||[]).map(id=>data.state.consumers?.[id]?.title||id).join(', ')||'Not assigned');
  const sources=Object.values(r.sources||{}).filter(source=>!source.withdrawn);
  if(sources.length){
    const section=details(root,'Sources · '+sources.length);
    for(const source of sources)section.append(E('p',[source.kind,source.version,source.relation,source.uri].join(' · '),'detail-fact'));
  }
  const reqNote=notesOf('requirement:'+selected).description;
  if(reqNote?.trim())root.append(E('p',reqNote,'node-description'));
  const base = {
    req: selected,
    revision: r.number
  };
  const acList=E('ol',undefined,'ac-list');
  acList.setAttribute('aria-label','Acceptance criteria');
  if(r.acs.length)root.append(E('h3','Acceptance criteria','ac-list-title'),acList);
  for (const a of r.acs) {
    const status = s?.acs[a.id];
    const item = E('li',undefined,'ac-item '+(status?.verification==='pass'?'pass':status?.verification==='fail'?'fail':'unverified'));
    // Заголовок одной строкой: текст условия, исход справа. Раньше значок
    // уезжал под заголовок и читался как ещё одно свойство, а не как ответ.
    applyObjectColor(item,'ac');
    const circle=statusCircle(acSignal(status,r,a.id,selected));circle.classList.add('ac-status');item.append(circle);
    const heading=E('div',undefined,'ac-heading');
    heading.append(E('p',a.text,'ac-text'));
    heading.append(itemActions('ac',a.id,a.text,selected));
    item.append(heading);
    if(a.description?.trim())item.append(E('p',a.description,'ac-description'));
    const acNote=notesOf('ac:'+selected+'/'+a.id).description;
    if(acNote?.trim())item.append(E('p',acNote,'ac-description'));
    // Свойства — подписанными парами, а не стопкой одинаковых абзацев:
    // причина, зависимость и решающий выглядели одинаково и сливались.
    const meta=E('dl',undefined,'ac-meta');
    const meta_row=(label,value)=>{meta.append(E('dt',label),E('dd',value));};
    if(current&&!status?.ready&&a.assessment?.reason)item.append(E('p',a.assessment.reason,'ac-reason'));
    // Зависимости — короткими ссылками по номеру, а не полными текстами
    // соседних условий: строка из трёх текстов нечитаема.
    if(a.depends_on?.length){
      const refs=E('span');
      a.depends_on.forEach((id,i)=>{
        const n=r.acs.findIndex(ac=>ac.id===id);
        const label=n>=0?'#'+(n+1):'unknown';
        const link=E('button',label,'ac-ref');
        link.type='button';
        link.title=n>=0?r.acs[n].text:'Another acceptance criterion';
        link.onclick=()=>{const t=acList.children[n];if(t){t.scrollIntoView({block:'center',behavior:'smooth'});t.classList.add('flash');setTimeout(()=>t.classList.remove('flash'),900);}};
        if(i)refs.append(document.createTextNode(' '));
        refs.append(link);
      });
      meta_row('Depends on',refs);
    }
    if(a.owner)meta_row('Owner',personLabel(a.owner));
    if(meta.children.length)item.append(meta);
    const linkedChecks=[];
    for(const [runId,run] of Object.entries(r.runs||{})){
      for(const check of run.plan||[])if(check.ac===a.id)linkedChecks.push({runId,run,check,result:run.results?.[check.id]});
    }
    if(linkedChecks.length){
      const checks=E('section',undefined,'ac-checks');
      checks.append(E('h4','Tests linked to this AC · '+linkedChecks.length,'ac-checks-title'));
      for(const {runId,run,check,result} of linkedChecks){
        const outcome=result?.outcome;
        const stale=result?.outcome==='pass'&&staleCheck(resultEvent(selected,r.number,runId,check.id)?.recorded_at);
        const tone=outcome==='fail'?'red':stale?'orange':outcome==='pass'?'green':result?'grey':run.finished?'grey':'yellow';
        const label=outcome==='pass'?(stale?'Passed · older than 7 days':'Passed'):outcome==='fail'?'Failed':outcome==='skip'?'Skipped':result?'Not checked':run.finished?'Awaiting result':'Running';
        const row=E('article',undefined,'ac-check');
        const heading=E('h5',undefined,'ac-check-heading');
        heading.append(statusCircle({tone,label}),E('span',(check.method==='manual'?'Manual':'Automated')+' test · '+run.scope,'ac-check-name'));
        row.append(heading);
        cardFact(row,'Expected',(check.expected||[]).join(', '));
        if(result)cardFact(row,'Observed',(result.observed||[]).join(', ')||'Nothing recorded');
        checks.append(row);
      }
      item.append(checks);
    }
    tasks(item,r,a.id,selected,current);
    if(current)commonNotes(item,'ac:'+a.id);
    acList.append(item);
  }
  tasks(root,r,null,selected,current);
  runs(root, r, base);
  comments(root, q, r, selected);
}

function personLabel(actor){
  if(actor===viewerActor)return 'You';
  return data.member_labels?.[actor]||{agent:'Agent',runner:'Test runner'}[data.state.members[actor]]||actor?.split('@')[0]||'Not assigned';
}
function eventDate(value){
  const date=new Date(value.replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isNaN(date.getTime())?'Date unavailable':new Intl.DateTimeFormat('en-GB',{day:'numeric',month:'short',year:'numeric',hour:'2-digit',minute:'2-digit'}).format(date);
}
function historicalCommand(kind,id,position){
  return data.history.findLast(e=>e.position<=position&&(
    kind==='requirement'?e.command.op==='revise'&&e.command.req===id:
    (e.command.op===kind||e.command.op==='edit_'+kind)&&e.command.id===id))?.command;
}
function historyTitle(kind,id,position){
  if(kind==='ac'){
    for(const event of [...data.history].reverse())if(event.position<=position&&event.command.op==='revise'){
      const ac=event.command.acs.find(a=>a.id===id);if(ac)return ac.text;
    }
  }
  return historicalCommand(kind,id,position)?.title||id;
}
function historyEvent(parent,event,q){
  const c=event.command;
  const names={set_domain_archived:c.archived?'Domain archived':'Domain restored',domain:'Domain created',capability:'Capability created',consumer:'Consumer created',edit_consumer:'Consumer updated',reorder_tree:'Order changed',edit_domain:'Domain updated',edit_capability:'Capability updated',confirm_tree:'Structure confirmed',revise:c.expected_revision?'Requirement updated':'Requirement created',link_source:'Source linked',withdraw_source:'Source withdrawn',comment:{decision:'Decision recorded',accept:'Suggestion accepted',change:'Changes requested',reject:'Suggestion declined',correction:'Reply corrected'}[c.kind]||'Comment added',assess:'Readiness reviewed',confirm:'Wording approved',task:'Task created',amend_task:'Task corrected',claim_task:'Task claimed',release_task:'Task released',review_task:'Task reviewed',retire_task:'Task retired',complete_task:'Task completed',plan:'Checks planned',result:'Check result recorded',finish:'Checks completed',accept:'Requirement accepted',revoke:'Acceptance withdrawn',issue:'Issue confirmed',resolve:'Issue resolved',withdraw_decision:'Decision withdrawn'};
  const entry=E('article',undefined,'history-entry');
  entry.append(E('time',eventDate(event.recorded_at),'history-time'),E('span',personLabel(event.actor),'history-author'));parent.append(entry);
  const body=E('div',undefined,'history-body');body.append(E('strong',names[c.op]||'Item updated','history-action'));entry.append(body);
  const fact=(label,value)=>{if(value!==undefined&&value!==null&&value!==''){const p=E('p');p.append(E('span',label,'history-label'),E('span',String(value)));body.append(p);}};
  const change=(label,before,after)=>{if(before===after)return;const block=E('span',undefined,'history-change');block.append(E('span',label+': ','history-label'));if(before)block.append(E('del',String(before),'history-before'));if(before&&after)block.append(E('span',' → '));if(after)block.append(E('ins',String(after),'history-after'));body.append(block);};
  const title=(kind,id)=>historyTitle(kind,id,event.position);
  const revision=q?.revisions.find(r=>r.number===c.revision);
  if(c.reason)fact('Reason',c.reason);
  if(c.op==='domain'||c.op==='capability'){
    fact('',c.title);if(c.domain)fact('Domain',title('domain',c.domain));if(c.owner)fact('Owner',personLabel(c.owner));
  }else if(c.op==='edit_domain'||c.op==='edit_capability'){
    const kind=c.op.slice(5),previous=historicalCommand(kind,c.id,event.position-1);
    change('Title',previous?.title,c.title);if(c.owner)change('Owner',personLabel(previous?.owner),personLabel(c.owner));
  }else if(c.op==='reorder_tree'){
    const id=parent.dataset.itemId;
    if(id&&c.ids.includes(id))body.append(E('p','Moved from position '+(c.expected.indexOf(id)+1)+' to '+(c.ids.indexOf(id)+1)+'.'));
  }else if(c.op==='revise'){
    const old=q.revisions[c.expected_revision-1];
    fact('Revision',c.expected_revision+1);change('Title',old?.title,c.title);change('Description',old?.body,c.body);
    if(old?.owner!==c.owner)change('Owner',old?personLabel(old.owner):null,personLabel(c.owner));
    if(JSON.stringify(old?.consumers||[])!==JSON.stringify(c.consumers||[]))change('Consumers',(old?.consumers||[]).map(id=>data.state.consumers?.[id]?.title||id).join(', '),(c.consumers||[]).map(id=>data.state.consumers?.[id]?.title||id).join(', '));
    const previous=historicalCommand('requirement',c.req,event.position-1);
    if(previous?.capability!==c.capability)change('Capability',previous?title('capability',previous.capability):null,title('capability',c.capability));
    for(const id of new Set([...(old?.acs||[]).map(a=>a.id),...c.acs.map(a=>a.id)])){
      if(parent.dataset.itemKind==='ac'&&id!==parent.dataset.itemId)continue;
      const before=old?.acs.find(a=>a.id===id),after=c.acs.find(a=>a.id===id);
      change(!before?'Acceptance criterion added':!after?'Acceptance criterion removed':'Acceptance criterion',before?.text,after?.text);
      if(after){
        if(before?.owner!==after.owner&&(before?.owner||after.owner))change('Owner · '+after.text,before?.owner?personLabel(before.owner):null,personLabel(after.owner));
        if(!!before?.before_launch!==!!after.before_launch)fact('Required before launch · '+after.text,after.before_launch?'Yes':'No');
        if(JSON.stringify(before?.depends_on||[])!==JSON.stringify(after.depends_on||[]))change('Dependencies · '+after.text,before?.depends_on?.map(id=>historyTitle('ac',id,event.position-1)).join('; '),after.depends_on?.map(id=>title('ac',id)).join('; ')||'None');
      }
    }
  }else if(c.op==='comment'){
    fact('Comment',c.text);if(c.ac)fact('Acceptance criterion',title('ac',c.ac));
    if(c.reply_to)fact('In reply to',q?.comments[c.reply_to]?.text);
  }else if(c.op==='link_source'){
    fact('Kind',c.kind);fact('Version',c.version);fact('Relation',c.relation);fact('Source',c.uri);
  }else if(c.op==='withdraw_source'){
    fact('Source',c.id);
  }else if(c.op==='assess'){
    fact('Acceptance criterion',title('ac',c.ac));fact('Outcome',c.assessment.ready?'Ready to work':'Waiting for a decision');fact('Explanation',c.assessment.reason);
    if(c.assessment.owner)fact('Waiting for',personLabel(c.assessment.owner));
  }else if(c.op==='task'){
    fact('Task',c.title);fact('Assigned to',personLabel(c.assignee));fact('Acceptance criteria',c.acs.map(id=>title('ac',id)).join('; '));
  }else if(c.op==='complete_task'){
    fact('Task',revision?.tasks[c.id]?.title);fact('Result',c.evidence);
  }else if(c.op==='plan'){
    fact('Scope',c.scope);fact('Checks',c.checks.length);
    for(const check of c.checks){fact(check.method==='manual'?'Manual check':'Automated check',title('ac',check.ac));fact('Expected',check.expected.join(', '));}
  }else if(c.op==='result'){
    const check=revision?.runs[c.run]?.plan.find(p=>p.id===c.evidence.check);
    if(check){fact('Acceptance criterion',title('ac',check.ac));fact('Method',check.method==='manual'?'Manual check':'Automated test');fact('Expected',check.expected.join(', '));}
    fact('Result',{pass:'Passed',fail:'Failed',skip:'Not checked'}[c.evidence.outcome]);fact('Observed',c.evidence.observed.join(', '));fact('Note',c.evidence.note);fact('Evidence',c.evidence.link);
  }else if(c.op==='issue'||c.op==='resolve'||c.op==='withdraw_decision'){
    if(c.ac)fact('Acceptance criterion',title('ac',c.ac));fact('Discussion',q?.comments[c.comment]?.text);
  }else if(c.op==='confirm_tree')fact('Decision','The product structure was approved.');
  if(c.revision)fact('Revision',c.revision);
}
function cardFact(root,label,value){
  if(value===undefined||value===null||value==='')return;
  const line=E('p',undefined,'detail-fact');
  line.append(E('span',label+': ','hint'),E('span',value));root.append(line);
}
function acTitle(r,id){return r.acs.find(a=>a.id===id)?.text||'Acceptance criterion';}
function tasks(root, r, acId, reqId, current) {
  const entries=Object.entries(r.tasks).filter(([,t])=>acId?t.acs.includes(acId):!t.acs.some(id=>r.acs.some(a=>a.id===id)));
  if(!entries.length)return;
  const d=details(root,(acId?'Tasks':'Historical tasks')+' · '+entries.length);
  for(const [id,t] of entries){
    const item=E('article',undefined,'detail-entry task-entry');applyObjectColor(item,'task');
    const taskStatus=t.retired?'Task no longer applies':t.review_required?'Review task against current REQ revision':t.done!==null?'Task completed':t.assignee?'In progress':'Open task';
    const heading=E('h3',t.title);heading.prepend(statusCircle({tone:t.retired?'grey':t.review_required?'purple':t.done!==null?'green':t.assignee?'yellow':'grey',label:taskStatus}));
    item.append(heading,E('p',t.assignee?personLabel(t.assignee):'Unclaimed','hint'));
    if(t.review_required)item.append(E('p','The REQ changed. Its owner must review this task against the current AC before work can be completed.','hint'));
    if(t.retired)cardFact(item,'No longer applies',t.retired);
    if(t.origin_revision&&t.origin_revision!==r.number)cardFact(item,'Created in revision',String(t.origin_revision));
    if(!acId)cardFact(item,'Acceptance criteria',t.acs.map(id=>acTitle(r,id)).join('; '));
    cardFact(item,'Contract',t.contract);
    cardFact(item,'Projects',t.projects.map(id=>data.state.projects[id]||id).join(', '));
    cardFact(item,'Work',({implementation:'Implementation',manual:'Manual action',decision:'Decision'})[t.purpose]);
    cardFact(item,'Result',t.done);if(current)commonNotes(item,id.includes(':')?id:'task:'+reqId+'/'+id);d.append(item);
  }
}
function runs(root, r, base) {
  if(!Object.keys(r.runs).length)return;
  const d=details(root,'Checks · '+Object.keys(r.runs).length);
  for(const [id,run] of Object.entries(r.runs)){
    const block=details(d,run.scope+' · '+(run.finished?'Finished':'Running'));
    const failed=Object.values(run.results).some(result=>result.outcome==='fail');
    const allReportedPassed=run.finished&&run.discovery_complete&&run.plan.length&&run.plan.every(check=>run.results[check.id]?.outcome==='pass');
    block.querySelector('summary').prepend(statusCircle({tone:failed?'red':!run.finished?'yellow':'grey',label:failed?'Checks failed':!run.finished?'Checks running':allReportedPassed?'All reported checks passed · see AC verdict':'Checks incomplete'}));
    for(const [component,version] of Object.entries(run.subject))cardFact(block,data.state.projects[component]||component,version);
    if(!run.discovery_complete)block.append(E('p','Check discovery is incomplete.','hint'));
    for(const c of run.plan){
      const result=run.results[c.id];
      const recorded=resultEvent(base.req,base.revision,id,c.id);
      const item=E('article',undefined,'detail-entry');
      item.append(E('h3',acTitle(r,c.ac)));
      const method=c.method==='manual'?'Manual check':'Automated test';
      const outcome=result?({pass:'Passed',fail:'Failed',skip:'Skipped'})[result.outcome]:'Awaiting result';
      const checkStatus=E('p',method+' · '+outcome,'hint');
      checkStatus.prepend(statusCircle({tone:result?.outcome==='pass'?'green':result?.outcome==='fail'?'red':'grey',label:outcome}));item.append(checkStatus);
      if(recorded)item.append(E('p',eventDate(recorded.recorded_at)+' · '+personLabel(recorded.actor),'hint'));
      cardFact(item,'Expected',c.expected.join(', '));
      if(result){
        cardFact(item,'Observed',result.observed.join(', ')||'Nothing recorded');
        cardFact(item,'Note',result.note);cardFact(item,'Evidence',result.link);
      }
      block.append(item);
    }
  }
}
function comments(root, q, r, reqId) {
  const entries=Object.entries(q.comments).filter(([,c])=>c.revision===r.number);
  if(!entries.length)return;
  const d=details(root,'Discussion · '+entries.length);
  const kinds={comment:'Comment',suggestion:'Suggestion',decision:'Decision',accept:'Suggestion accepted',change:'Changes requested',reject:'Suggestion declined',correction:'Correction'};
  for(const [id,c] of entries){
    const item=E('article',undefined,'detail-entry');
    const event=data.history.find(e=>e.command.op==='comment'&&e.command.req===reqId&&e.command.id===id);
    item.append(E('p',[event&&eventDate(event.recorded_at),personLabel(c.actor),kinds[c.kind]||'Comment',c.withdrawn&&'Withdrawn'].filter(Boolean).join(' · '),'hint'));
    if(c.reply_to)cardFact(item,'In reply to',q.comments[c.reply_to]?.text);
    if(c.ac)cardFact(item,'Acceptance criterion',acTitle(r,c.ac));
    item.append(E('p',c.text));d.append(item);
  }
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
  render();
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
async function openRequirement(id) {
  if(data.lazy&&!data.state.requirements[id])await ensureFilterIndex();
  filters={domain:'',status:'',person:''};render();
  const req=[...$('tree').querySelectorAll('.tree-requirement')].find(e=>e.dataset.requirement===id);
  if(!req)return;
  for(let node=req;node;node=node.parentElement)if(node.tagName==='DETAILS')node.open=true;
  req.scrollIntoView({block:'start'});
}

function copyButton(kind,id,title) {
  const b=E('button','', 'copy-context');b.type='button';
  b.onclick=async()=>{
    const systemId=data.system_ids?.[kind]?.[id];
    if(!systemId){notify(Error('System ID unavailable. Refresh the page.'));return;}
    const text=systemId;
    try{
      if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
      await navigator.clipboard.writeText(text);
      notify('Copied ID for your agent.');
    }catch{
      const dialog=E('dialog',undefined,'copy-dialog');
      dialog.setAttribute('aria-label','Copy for your agent');
      const area=E('input');area.readOnly=true;area.value=text;area.setAttribute('aria-label','ID');
      const copy=E('button','Copy ID');copy.type='button';
      copy.onclick=()=>{
        area.focus();area.select();
        let copied=false;
        try{copied=document.execCommand('copy');}catch{}
        if(copied){dialog.close();notify('Copied ID for your agent.');}
        else notify(Error('Touch and hold the selected ID, then choose Copy.'));
      };
      dialog.append(E('h2','Copy for your agent'),E('p','Tap Copy ID, or touch and hold the selected text and choose Copy.'),area,copy,button('Close',()=>dialog.close()));
      dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();area.focus();area.select();
    }
  };
  const svgNS='http://www.w3.org/2000/svg';
  const icon=document.createElementNS(svgNS,'svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'14',height:'14',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))icon.setAttribute(key,value);
  const square=document.createElementNS(svgNS,'rect');
  for(const [key,value] of Object.entries({x:'8',y:'8',width:'12',height:'12',rx:'2'}))square.setAttribute(key,value);
  const back=document.createElementNS(svgNS,'path');back.setAttribute('d','M16 8V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h4');
  icon.append(square,back);
  const face=icon.cloneNode(false);face.setAttribute('width','18');face.setAttribute('height','18');face.classList.add('agent-face');
  for(const [tag,attrs] of [
    ['rect',{x:'4',y:'7',width:'16',height:'13',rx:'4'}],
    ['path',{d:'M12 7V4m-2 0h4M1 12v4m22-4v4M9 16h6'}],
    ['circle',{cx:'8.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}],
    ['circle',{cx:'15.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}]
  ]){const part=document.createElementNS(svgNS,tag);for(const [key,value] of Object.entries(attrs))part.setAttribute(key,value);face.append(part);}
  b.append(icon,face);
  b.title='Copy for agent';
  b.setAttribute('aria-label','Copy '+kind+' ID: '+id);
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}

function orderedEntries(kind,parent=null) {
  const map=kind==='domain'?data.state.domains:kind==='capability'?data.state.capabilities:data.state.requirements;
  const ids=Object.keys(map).filter(id=>kind==='domain'?!map[id].archived:(kind==='capability'?map[id].domain:map[id].capability)===parent).sort();
  const key=kind+(parent?':'+parent:'');
  const saved=(data.state.tree_order?.[key]||[]).filter(id=>ids.includes(id));
  return [...saved,...ids.filter(id=>!saved.includes(id))].map(id=>[id,map[id]]);
}
function itemActions(kind,id,title,reqId=null,revision=null){
  const actions=E('div',undefined,'node-actions');
  // Номер ревизии стоит ВПЛОТНУЮ к истории: это один вопрос — «которая это
  // редакция и что было до неё». Отдельной строкой под заголовком он читался
  // как свойство требования, хотя на деле это отметка версии.
  actions.append(copyButton(kind,id,title));
  actions.append(notesButton(kind,id,title,reqId));
  const object=button('Links',()=>showCommonObject(kind+':'+id),'object-button');object.title='Read document revisions and links';object.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();});actions.append(object);
  const files=filesButton(kind,id,title,reqId);
  if(files)actions.append(files);
  if(revision!==null)actions.append(E('span','rev. '+revision,'rev-chip'));
  const history=button('',()=>showHistory(kind,id,title,reqId),'history-button');history.title='View history';history.setAttribute('aria-label','View '+kind+' history: '+id);
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'16',height:'16',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(key,value);
  const path=document.createElementNS(icon.namespaceURI,'path');path.setAttribute('d','M3 11a9 9 0 1 1 2.7 7.4M3 4v7h7M12 7v5l3 2');icon.append(path);history.append(icon);
  history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});actions.append(history);return actions;
}
async function showHistory(kind,id,title,reqId){
  const historyReq=reqId||(kind==='requirement'?id:null);
  if(data.lazy&&(!data.fullHistory||historyReq&&!fullHistoryRequirements.has(historyReq))){
    const part=await boardPart('history',historyReq||id);
    data.history=part.history;
    if(part.requirement){data.state.requirements[historyReq]=part.requirement;fullHistoryRequirements.add(historyReq);}
    data.fullHistory=true;
    indexStatusEvents();
  }
  const q=data.state.requirements[kind==='requirement'?id:reqId];
  const created=data.history.find(e=>e.command.op===kind&&e.command.id===id)?.position||0;
  const events=data.history.filter(event=>{
    if(event.position<created)return false;
    const c=event.command;
    if(c.op==='reorder_tree')return c.kind===kind&&c.ids.includes(id)&&c.expected.indexOf(id)!==c.ids.indexOf(id);
    if(kind==='requirement')return c.req===id;
    if(kind!=='ac')return ((c.op===kind||c.op==='edit_'+kind||(kind==='domain'&&c.op==='set_domain_archived'))&&c.id===id)||c.op==='confirm_tree';
    if(c.req!==reqId)return false;
    if(c.op==='revise')return c.acs.some(a=>a.id===id)||!!q.revisions[c.expected_revision-1]?.acs.some(a=>a.id===id);
    const revision=q.revisions.find(r=>r.number===c.revision);
    if(!revision?.acs.some(a=>a.id===id))return false;
    if(c.ac)return c.ac===id;
    if(c.op==='task')return c.acs.includes(id);
    if(c.op==='complete_task')return revision.tasks[c.id]?.acs.includes(id);
    if(c.op==='plan')return c.checks.some(check=>check.ac===id);
    if(c.op==='result')return revision.runs[c.run]?.plan.some(check=>check.id===c.evidence.check&&check.ac===id);
    if(c.op==='resolve')return revision.issues[c.id]?.ac===id;
    // Событие попадает в историю AC, только если НАЗЫВАЕТ его. Раньше здесь
    // стояло true, и всё, что относилось к ревизии требования и не называло
    // ни одного условия, показывалось в истории КАЖДОГО его условия — история
    // AC читалась как история REQ.
    return false;
  });
  const dialog=E('dialog',undefined,'history-dialog');dialog.setAttribute('aria-label','History: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');heading.append(E('small','History'),E('h2',title));
  heading.tabIndex=-1;heading.autofocus=true;
  if(kind==='requirement'){
    const card=[...document.querySelectorAll('[data-requirement]')].find(node=>node.dataset.requirement===id);
    const body=card?.querySelector('.requirement-detail');
    const revisions=E('select',undefined,'history-revisions');
    revisions.setAttribute('aria-label','Requirement revision');
    for(const revision of [...q.revisions].reverse()){
      const option=E('option','Revision '+revision.number+(revision===q.revisions.at(-1)?' · Latest':'')+(revision.reason?' · '+revision.reason:''));
      option.value=revision.number;revisions.append(option);
    }
    revisions.value=body?.dataset.revision||q.revisions.at(-1).number;
    revisions.onchange=()=>{
      if(!body)return;
      requirementDetails(body,id,Number(revisions.value));
      card.open=true;dialog.close();
      body.scrollIntoView({block:'nearest'});
    };
    heading.append(revisions);
  }
  const close=button('',()=>dialog.close(),'history-close');close.setAttribute('aria-label','Close history');
  const closeIcon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'18',height:'18',fill:'none',stroke:'currentColor','stroke-width':'1.8','stroke-linecap':'round','aria-hidden':'true'}))closeIcon.setAttribute(key,value);
  const closePath=document.createElementNS(closeIcon.namespaceURI,'path');closePath.setAttribute('d','M6 6l12 12M6 18L18 6');closeIcon.append(closePath);close.append(closeIcon);header.append(heading,close);dialog.append(header);
  const content=E('div',undefined,'history-timeline');content.dataset.itemId=id;content.dataset.itemKind=kind;
  for(const event of events.reverse())historyEvent(content,event,q);
  if(!content.childNodes.length)content.append(E('p','No changes recorded yet.','hint'));
  dialog.append(content);dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}

/* ---- Заметки: пояснение, обсуждение и вложения на любом объекте ---- */

// Адрес объекта. Тот же вид, что понимает сервер, — вид:идентификатор.
function address(kind,id,reqId){return ['ac','point'].includes(kind)?kind+':'+reqId+'/'+id:kind+':'+id;}
function notesOf(target){return (data?.state?.notes||{})[target]||{description:'',comments:{},attachments:{}};}
async function ensureNotes(target){
  if(!data.lazy||Object.hasOwn(data.state.notes,target))return;
  const part=await boardPart('notes',target);
  data.state.notes[target]=part.notes;
}
function liveNotes(target){
  const n=notesOf(target);
  return {
    description:n.description||'',
    comments:Object.entries(n.comments||{}).filter(([,c])=>!c.withdrawn),
    attachments:Object.entries(n.attachments||{}).filter(([,a])=>!a.removed),
  };
}
// Описание объекта может жить в нём самом (домен, капабилити) — тогда
// показываем его, а не пустую заметку. Одно поле в интерфейсе, два места
// хранения: человеку разница не нужна, ему нужен текст.
function describedText(kind,id,reqId,target){
  if(kind==='domain')return data.state.domains[id]?.description||'';
  if(kind==='capability')return data.state.capabilities[id]?.description||'';
  return notesOf(target).description||'';
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
function notesButton(kind,id,title,reqId){
  const target=address(kind,id,reqId);
  const n=liveNotes(target);
  const count=n.comments.length;
  const b=button('',async()=>{await ensureNotes(target);showNotes(kind,id,title,reqId);},'notes-button');
  if(count)b.classList.add('has-notes');
  b.disabled=!data.lazy&&count===0;
  b.title=count?'Comments · '+count:data.lazy?'Open notes':'No comments';
  b.setAttribute('aria-label','Notes on '+kind+' '+id+(count?': '+count:''));
  b.append(svgIcon('M21 11.5a8.4 8.4 0 0 1-9 8.4 9 9 0 0 1-3.8-.9L3 20.5l1.6-4.6A8.4 8.4 0 0 1 3.5 11 8.5 8.5 0 0 1 12 3a8.5 8.5 0 0 1 9 8.5Z'));
  if(count)b.append(E('span',String(count),'notes-count'));
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}
function filesButton(kind,id,title,reqId){
  const target=address(kind,id,reqId),attachments=liveNotes(target).attachments;
  const b=button('',async()=>{await ensureNotes(target);showFiles(title,liveNotes(target).attachments);},'files-button');
  b.dataset.target=target;
  b.disabled=!data.lazy&&attachments.length===0;
  if(attachments.length)b.classList.add('has-files');
  b.title=attachments.length?'Files · '+attachments.length:data.lazy?'Open files':'No files';
  b.setAttribute('aria-label','Files on '+kind+' '+id+': '+attachments.length);
  b.setAttribute('aria-haspopup','dialog');
  b.append(svgIcon(FILE_ICON));
  if(attachments.length)b.append(E('span',String(attachments.length),'files-count'));
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}
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
function showFiles(title,attachments){
  const dialog=E('dialog',undefined,'notes-dialog files-dialog');
  dialog.setAttribute('aria-label','Files: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');
  heading.append(E('small','Files · '+attachments.length),E('h2',title));
  const close=button('',()=>dialog.close(),'history-close');
  close.setAttribute('aria-label','Close files');close.append(svgIcon('M6 6l12 12M6 18L18 6',18));
  header.append(heading,close);dialog.append(header,attachmentList(attachments));
  dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}
function fileSize(bytes){
  if(bytes<1024)return bytes+' B';
  if(bytes<1024*1024)return Math.round(bytes/1024)+' KB';
  return (bytes/(1024*1024)).toFixed(1)+' MB';
}
function showNotes(kind,id,title,reqId){
  const target=address(kind,id,reqId);
  const n=liveNotes(target);
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label','Notes: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');
  heading.append(E('small','Notes'),E('h2',title));
  const close=button('',()=>dialog.close(),'history-close');close.setAttribute('aria-label','Close notes');
  close.append(svgIcon('M6 6l12 12M6 18L18 6',18));header.append(heading,close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);

  /* Описание */
  const descSection=E('section',undefined,'notes-section');
  descSection.append(E('h3','Description','notes-heading'));
  const descText=describedText(kind,id,reqId,target);
  descSection.append(E('p',descText||'No description yet.',descText?'notes-description':'hint'));
  if(kind==='ac'){
    const own=currentAc(reqId,id)?.description;
    if(own)descSection.append(E('p','From the revision: '+own,'hint'));
  }
  body.append(descSection);

  /* Обсуждение */
  const talk=E('section',undefined,'notes-section');
  talk.append(E('h3','Discussion'+(n.comments.length?' · '+n.comments.length:''),'notes-heading'));
  const thread=E('div',undefined,'notes-thread');
  const ordered=n.comments.sort((a,b)=>String(a[1].at).localeCompare(String(b[1].at)));
  for(const [,c] of ordered){
    const item=E('article',undefined,'note');
    const meta=E('div',undefined,'note-meta');
    meta.append(E('span',c.actor,'note-actor'),E('time',when(c.at),'note-at'));
    item.append(meta,E('p',c.text,'note-text'));
    thread.append(item);
  }
  if(!ordered.length)thread.append(E('p','Nothing said here yet.','hint'));
  talk.append(thread);
  body.append(talk);

  dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}
function currentAc(reqId,acId){
  return data.state.requirements[reqId]?.revisions.at(-1)?.acs.find(a=>a.id===acId);
}

/* Common objects use the same cards for every configured type. */
async function objectPart(params){return api('workspaces/'+encodeURIComponent(ws)+'/objects?'+new URLSearchParams(params));}
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
  const items=part.items.filter(item=>params.kind||(params.parent&&params.parent!=='*')||item.kind!=='note');
  for(const item of items){
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
  if(!items.length&&!part.next_cursor&&!root.querySelector('.common-object'))root.append(E('p',params.kind&&data.objectTypes[params.kind]?'No '+data.objectTypes[params.kind].title.toLowerCase()+' here.':'No objects here yet.','hint'));
  if(part.next_cursor){const more=button('Load more',async()=>{more.disabled=true;try{const next=await objectPart({...params,cursor:part.next_cursor});if(generation!==loadNumber||!root.isConnected)return;more.remove();appendObjectPage(root,next,params,generation);}catch(e){more.disabled=false;notify(e);}},'text-button');root.append(more);}
  if(!params.parent&&!params.kind&&$('tree').querySelector('.collection-tabs'))rootCollectionTabs($('tree'));
}
async function renderObjectTree(){
  const root=$('tree');root.replaceChildren();
  const generation=loadNumber;
  const configuration=await api('workspaces/'+encodeURIComponent(ws)+'/tree');if(generation!==loadNumber)return;
  data.objectTypes=configuration.schema.types;
  const intro=E('div',undefined,'tree-intro');intro.append(E('h1',data.state.title),E('p','Open an object to read its details and children.','hint'));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(data.launch.blocker_count??data.launch.blockers?.length),'hint'));
  root.append(intro);
  const contents=E('div');
  const controls=commonObjectFilters(root,contents);root.append(contents);
  for(const select of controls.querySelectorAll('select'))select.disabled=true;
  contents.append(E('p','Loading objects…','object-loading hint'));
  await appendObjectChildren(contents);
  if(generation!==loadNumber)return;
  for(const select of controls.querySelectorAll('select'))select.disabled=false;
  for(const kind of inboxKinds())commonNotes(root,undefined,kind);
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
  const kind=picker('Object type',[['','All types'],...Object.entries(data.objectTypes||{}).map(([id,type])=>[id,type.title])]);
  const owner=picker('Object owner',[['','All owners'],...Object.keys(data.state.members).map(id=>[id,personLabel(id)])]);
  const status=picker('Object status',[['','All statuses'],['green','Checks passed'],['red','Checks failed'],['purple','Dependency review'],['yellow','Waiting or in progress'],['grey','Unchecked or completed']]);
  const update=async()=>{const mine=++request,generation=loadNumber;const selected={kind:kind.value,owner:owner.value,status:status.value};const tabs=root.querySelector('.collection-tabs');if(tabs)tabs.hidden=Boolean(selected.kind||selected.owner||selected.status);const holder=E('div');contents.replaceChildren(holder);holder.append(E('p','Loading objects…','object-loading hint'));try{
    if(!selected.kind&&!selected.owner&&!selected.status){await appendObjectChildren(holder);return;}
    const params={parent:'*',limit:'100'};if(selected.kind)params.kind=selected.kind;if(inputQueue(selected.kind))params.note_status='unprocessed';let cursor;const items=[];
    do{const page=await objectPart(cursor?{...params,cursor}:params);if(mine!==request||generation!==loadNumber||!controls.isConnected)return;items.push(...page.items);cursor=page.next_cursor;}while(cursor);
    const rows=items.filter(item=>(selected.kind||!inputQueue(item.kind))&&(!selected.owner||item.owner===selected.owner)&&(!selected.status||item.signal?.tone===selected.status));
    holder.replaceChildren(...rows.map(commonObjectCard));if(!rows.length)holder.append(E('p','No objects match these filters.','hint'));
  }catch(e){if(mine===request)notify(e);}};
  for(const select of [kind,owner,status])select.onchange=update;root.append(controls);return controls;
}
function commonNotes(root,relatedTo,kind='note'){
  const label=data.objectTypes?.[kind]?.title||kind;
  const title=relatedTo?'Related '+label.toLowerCase():label;
  const queue=inputQueue(kind);
  const section=E('section');section.append(E('header',undefined,'collection-heading'));root.append(section);section.classList.add('note-section');applyObjectColor(section,kind);
  collectionHeading(section,title,relatedTo?'Read only.':data.objectTypes?.[kind]?.description||(queue?'Unprocessed information to review and apply. Read only.':'Read only.'),relatedTo?undefined:label);
  if(!relatedTo){section.classList.add('root-collection');section.dataset.collectionKind=kind;}
  const summary=section.querySelector(':scope > header');
  const body=E('div',undefined,'note-section-body');section.append(body);
  let loaded=false,loading=false,request=0;
  let all;
  if(!relatedTo&&queue){
    const controls=E('div',undefined,'note-controls');
    const label=E('label',undefined,'note-all');all=E('input');all.type='checkbox';all.setAttribute('aria-label','All '+label.toLowerCase());label.append(all,E('span','All'));controls.append(label);summary.append(controls);
  }
  const rows=E('div',undefined,'note-items');body.append(rows);
  const loadNotes=async()=>{
    const mine=++request;loaded=false;loading=true;
    const holder=E('div');holder.append(E('p','Loading '+label.toLowerCase()+'…','object-loading hint'));rows.replaceChildren(holder);
    try{await appendObjectChildren(holder,'*',kind,relatedTo?{related_to:relatedTo}:queue&&!all?.checked?{note_status:'unprocessed'}:{});if(mine===request&&section.isConnected){
      loaded=true;const empty=!holder.querySelector('.common-object');section.classList.toggle('empty-notes',empty);
      summary.title=empty?(relatedTo?'No related notes.':all.checked?'No notes.':'No unprocessed notes.'):'Read notes';
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
  if(item.kind==='note')summary.append(E('span',item.note_status==='processed'?'Processed':'Unprocessed','note-state'));
  const actions=E('span',undefined,'item-actions');
  const copy=button('COPY',async()=>{await copyAction(copy,item.item_id||item.address);},'copy');copy.type='button';copy.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();});
  const history=button('Revisions',()=>showCommonObject(item.address,true),'history-button');history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});actions.append(copy,history);summary.append(actions);card.append(summary);
  const body=E('div',undefined,'object-body');card.append(body);enableBranchMotion(card);let loaded=false;
  card.addEventListener('toggle',async()=>{if(!card.open||loaded)return;loaded=true;const generation=loadNumber;body.append(E('p','Loading object…','object-loading hint'));try{const detail=await objectPart({address:item.address});if(generation!==loadNumber||!card.isConnected)return;resizeBranch(card,()=>{body.replaceChildren();fillCommonObject(body,detail);});const children=E('div',undefined,'object-children');body.append(children);await appendObjectChildren(children,item.address);}catch(e){loaded=false;body.replaceChildren();notify(e);}});
  return card;
}
function fillCommonObject(body,detail){
  const r=detail.document,d=r.data;
  const historical=detail.historical??r.revision!==detail.current_revision;
  if(historical)body.append(E('p','Saved document revision. Current status, comments, files and incoming links are not shown.','hint'));
  if(r.present===false)body.append(E('p','Historical object. This object is absent from the current verification revision.','dependency-review'));
  body.append(E('p','Revision '+r.revision+' · Owner: '+personLabel(d.owner||'Unassigned'),'node-meta'));
  if(detail.process?.status)body.append(E('p','Status: '+(typeof detail.process.status==='string'?detail.process.status:detail.signal?.label||'Current'),'node-meta'));
  if(detail.review_required)body.append(E('p','A dependency changed. Review the linked revision.','dependency-review'));
  for(const [key,value] of Object.entries(d)){
    if(['title','owner'].includes(key)||(detail.kind==='note'&&key==='status')||value===null||value===''||(Array.isArray(value)&&value.length===0))continue;
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
  for(const comment of Object.values(annotations.comments||{})){if(comment.withdrawn)continue;const row=E('article',undefined,'note');row.append(E('p',personLabel(comment.actor)+' · '+when(comment.at),'note-meta'),E('p',comment.text,'note-text'));body.append(row);}
  const files=Object.entries(annotations.attachments||{}).filter(([,file])=>!file.removed);if(files.length)body.append(attachmentList(files));
  if(!historical)for(const kind of inboxKinds())commonNotes(body,detail.address,kind);
  if(detail.kind==='requirement'&&!historical){body.append(button('Checks and tasks',async()=>{const part=await boardPart('requirement',detail.id);data.state.requirements[detail.id]=part.requirement;data.summary[detail.id]=part.summary;Object.assign(data.state.notes,part.notes);Object.assign(data.state.consumers,part.consumers);Object.assign(data.state.projects,part.projects);Object.assign(data.system_ids.ac,part.system_ids?.ac||{});const content=E('div');body.append(content);requirementDetails(content,detail.id);},'text-button'));}
}
async function showCommonObject(address,history=false,revision){
  const generation=loadNumber;
  const detail=await objectPart(revision?{address,revision}:{address});if(generation!==loadNumber)return;
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label',detail.document.data.title);
  const header=E('div',undefined,'history-dialog-header');header.append(E('h2',detail.document.data.title));const close=button('Close',()=>dialog.close(),'history-close');header.append(close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);fillCommonObject(body,detail);
  if(history){const select=E('select');select.setAttribute('aria-label','Object revision');for(let n=detail.current_revision;n>0;n--){const option=E('option','Revision '+n);option.value=n;select.append(option);}header.append(select);let request=0;select.onchange=async()=>{const mine=++request;try{const old=await objectPart({address,revision:select.value});if(mine!==request||!dialog.isConnected)return;body.replaceChildren();fillCommonObject(body,old);}catch(e){notify(e);}};}
  dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();
}
