/* SNAPSHOT */
let ws = '', data = null, viewerActor = '';
let orderSaving=false,draggedNode=null,orderMenu=null,filterMenu=null;
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
let noticeTimer;
function notify(e) {
  clearTimeout(noticeTimer);
  $('notice').textContent=e?.message||String(e||'');
  $('notice').dataset.kind=e instanceof Error?'error':'success';
  if(e && !(e instanceof Error))noticeTimer=setTimeout(()=>{$('notice').textContent='';},4500);
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
  $('workspaces').disabled = list.length === 0;
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
async function load() {
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)));
  if (request !== loadNumber) return;
  data = next;
  render();
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
const statusLabels={waiting:'Waiting for decision',available:'Ready to work',progress:'In progress',ready:'Awaiting acceptance',pass:'Accepted',fail:'Checks failed'};
let filters={domain:'',status:'',person:''};
let filterWorkspace=null,statusCounts={};
function requirementStatus(id) {
  const r=data.state.requirements[id].revisions.at(-1),s=data.summary[id];
  if(Object.values(s.acs).some(a=>a.verification==='fail'))return 'fail';
  if(s.accepted)return 'pass';
  if(s.ready_for_acceptance)return 'ready';
  if(!r.confirmed||!r.acs.length||Object.values(s.acs).some(a=>!a.ready))return 'waiting';
  if(Object.keys(r.runs).length||Object.values(r.tasks).some(t=>t.purpose!=='decision'))return 'progress';
  return 'available';
}
function matchesPerson(id) {
  const actor=filters.person==='@me'?viewerActor:filters.person;
  if(!actor)return true;
  const r=data.state.requirements[id].revisions.at(-1);
  return r.owner===actor||Object.values(r.tasks).some(t=>!t.done&&t.assignee===actor)||
    r.acs.some(a=>!data.summary[id].acs[a.id]?.ready&&(a.decider===actor||a.assessment?.recipient===actor));
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
  text.append(E('small',caption),selected);
  const chevron=E('span','⌄','picker-chevron');chevron.setAttribute('aria-hidden','true');trigger.append(mark,text,chevron);host.append(trigger);
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
  if(filters.domain&&!data.state.domains[filters.domain])filters.domain='';
  if(filters.status&&!statusLabels[filters.status])filters.status='';
  const toolbar=E('section',undefined,'tree-toolbar');toolbar.setAttribute('aria-label','Filter product tree');
  const fields=E('div',undefined,'filter-fields');
  if(filters.person&&filters.person!=='@me'&&!data.state.members[filters.person])filters.person='';
  fields.append(filterPicker('domain','Filter by domain','Domain',[
    {id:'',title:'All domains',hint:String(Object.keys(data.state.domains).length)},
    ...orderedEntries('domain').map(([id,d])=>({id,title:d.title,hint:String(Object.values(data.state.capabilities).filter(c=>c.domain===id).length)+' capabilities'}))
  ],filters.domain,value=>{filters.domain=value;applyFilters();}));
  const people=[{id:'',title:'Everyone',hint:'All responsibilities'}];
  if(viewerActor)people.push({id:'@me',title:'Assigned to me',hint:'My responsibilities'});
  for(const [id,role] of Object.entries(data.state.members))people.push({id,title:id===viewerActor?'Me':id,hint:role});
  fields.append(filterPicker('person','Filter by person','Assigned to',people,filters.person,value=>{filters.person=value;applyFilters();}));
  fields.prepend(fields.lastElementChild);
  toolbar.append(fields);
  fields.append(filterPicker('status','Filter by status','Status',[
    {id:'',title:'All statuses',hint:()=>Object.values(statusCounts).reduce((a,b)=>a+b,0)+' requirements'},
    ...Object.entries(statusLabels).map(([id,title])=>({id,title,hint:()=>(statusCounts[id]||0)+' requirements'}))
  ],filters.status,value=>{filters.status=value;applyFilters();}));
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
  $('filter-results').textContent=shown+' of '+Object.keys(data.state.requirements).length+' requirements · '+visibleDomains+' domains';
  $('filter-empty').hidden=visibleDomains>0||!Object.keys(data.state.domains).length;
}
function render() {
  closeFilterMenu();
  const t = $('tree');
  const opened=new Set([...t.querySelectorAll('details[data-tree-node][open]')].map(d=>d.dataset.treeNode));
  t.replaceChildren();
  const intro=E('div',undefined,'tree-intro');
  intro.append(E('p','Product tree','tree-eyebrow'),E('h1',data.state.title));
  intro.append(E('p',`${Object.keys(data.state.domains).length} domains · ${Object.keys(data.state.capabilities).length} capabilities · ${Object.keys(data.state.requirements).length} requirements`,'node-stats'));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+data.launch.blockers.length,'hint'));
  t.append(intro);
  filterControls(t);
  for (const [did,d] of orderedEntries('domain')) {
    const group=E('details',undefined,'tree-domain');group.dataset.domain=did;group.dataset.treeNode='domain:'+did;group.open=opened.has(group.dataset.treeNode);
    const heading=E('summary',undefined,'title-row');
    heading.append(E('span',d.title,'node-title'),E('small','DOMAIN','type-label'),E('span',d.confirmed?'Confirmed':'Draft','badge '+(d.confirmed?'confirmed':'waiting')),copyButton('domain',did,d.title));group.append(heading);addOrdering(group,heading,'domain',null,did);
    const body=E('div',undefined,'domain-body');body.append(E('p','Decision owner: '+d.authority,'node-meta'));group.append(body);
    for(const [cid,c] of orderedEntries('capability',did)){
      const cap=E('details',undefined,'tree-capability');cap.dataset.capability=cid;cap.dataset.treeNode='capability:'+cid;cap.open=opened.has(cap.dataset.treeNode);
      const head=E('summary',undefined,'title-row');head.append(E('span',c.title,'node-title'),E('small','CAPABILITY','type-label'),copyButton('capability',cid,c.title));cap.append(head);addOrdering(cap,head,'capability',did,cid);
      const reqs=orderedEntries('requirement',cid);
      if(!reqs.length)cap.append(E('p','No requirements yet.','empty-state'));
      for(const [id,q] of reqs){
        const r=q.revisions.at(-1),tone=requirementStatus(id);
        const req=E('details',undefined,'tree-requirement '+tone);req.dataset.requirement=id;req.dataset.treeNode='requirement:'+id;req.open=opened.has(req.dataset.treeNode);
        const reqHead=E('summary',undefined,'title-row');reqHead.append(E('span',r.title,'node-title'),E('small','REQ','type-label'),E('span',statusLabels[tone],'badge '+tone),copyButton('requirement',id,r.title));
        const content=E('div',undefined,'requirement-detail');req.append(reqHead,content);addOrdering(req,reqHead,'requirement',cid,id);
        req.addEventListener('toggle',()=>{if(req.open&&!content.childNodes.length)requirementDetails(content,id);});
        if(req.open)requirementDetails(content,id);
        cap.append(req);
      }
      appendNodeHistory(cap,'capability',cid);
      body.append(cap);
    }
    appendNodeHistory(body,'domain',did);
    t.append(group);
  }
  if(!Object.keys(data.state.domains).length)t.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
  const empty=E('p','No requirements match these filters.','filter-empty');empty.id='filter-empty';empty.hidden=true;t.append(empty);
  applyFilters(false);
  intake(t);
}
function requirementDetails(root, selected, revisionNumber=null) {
  root.replaceChildren();
  const q = data.state.requirements[selected],
    r = q.revisions.find(r => r.number === revisionNumber) || q.revisions.at(-1),
    current = r === q.revisions.at(-1),
    s = current ? data.summary[selected] : null;
  if(current)root.dataset.status=requirementStatus(selected);
  if(!current)root.append(E('h3',r.title));
  root.append(E('p',selected+' · revision '+r.number+' · owner '+r.owner,'hint'),E('p',r.body));
  const revisions = E('select');
  for (const old of q.revisions) {
    const o = E('option', 'Revision ' + old.number + ' · ' + old.reason);
    o.value = old.number;
    revisions.append(o);
  }
  revisions.setAttribute('aria-label','Requirement revision');
  revisions.value = r.number;
  revisions.onchange = e => {
    requirementDetails(root,selected,+e.target.value);
  };
  root.append(revisions);
  root.append(E('p',current?statusLabels[requirementStatus(selected)]:'Previous revision. See its checks and history for evidence.',current?'badge '+requirementStatus(selected):'hint'));
  const base = {
    req: selected,
    revision: r.number
  };
  for (const a of r.acs) {
    const status = s?.acs[a.id];
    const item = E('article',undefined,'ac-card '+(status?.verification==='pass'?'pass':status?.verification==='fail'?'fail':'unverified'));
    const heading=E('div',undefined,'ac-heading');heading.append(E('h3',a.text),copyButton('ac',a.id,a.text));item.append(heading);
    item.append(E('small','AC · '+a.id), E('span', status?.verification || 'See history', 'badge ' + (status?.verification || '')), E('p', status?.ready ? 'Ready to work' : a.assessment?.reason || 'Awaiting review'));
    if (a.before_launch) item.append(E('small', 'Required before launch'));
    if (a.depends_on?.length) item.append(E('p','Depends on: '+a.depends_on.join(', ')));
    if (a.decider) item.append(E('p', 'Decision: ' + a.decider));
    root.append(item);
  }
  tasks(root, r);
  runs(root, r, base);
  comments(root, q, r);
  appendNodeHistory(root,'requirement',selected,q);
}

function historyEvent(parent, event, q) {
  const names = {
    reorder_tree: 'Order changed',
    edit_domain: 'Domain updated',
    edit_capability: 'Capability updated',
    confirm_tree: 'Structure confirmed',
    revise: 'Revision saved',
    comment: 'Discussion reply',
    assess: 'Readiness assessed',
    confirm: 'Wording confirmed',
    task: 'Task created',
    complete_task: 'Task completed',
    plan: 'Check plan sealed',
    result: 'Check result',
    finish: 'Run completed',
    accept: 'Result accepted',
    revoke: 'Acceptance revoked',
    issue: 'Issue confirmed',
    resolve: 'Fix confirmed',
    withdraw_decision: 'Decision withdrawn'
  };
  const c = event.command,
    d = details(parent, event.recorded_at + ' · ' + event.actor + ' · ' + (names[c.op] || c.op));
  if(c.op==='reorder_tree'){d.append(E('p','Before: '+c.expected.join(' → ')),E('p','After: '+c.ids.join(' → ')));}
  else if (c.op === 'revise') {
    const old = q.revisions[c.expected_revision - 1];
    d.append(E('p', 'Reason: ' + c.reason), E('p', 'Wording: ' + (old?.title || '—') + ' → ' + c.title));
    if (old?.body !== c.body) d.append(E('p', 'Context: ' + (old?.body || '—') + ' → ' + c.body));
    const ids = new Set([...(old?.acs || []).map(a => a.id), ...c.acs.map(a => a.id)]);
    for (const id of ids) {
      const a = old?.acs.find(a => a.id === id),
        b = c.acs.find(a => a.id === id);
      if (JSON.stringify(a) !== JSON.stringify(b)) d.append(E('p', id + ': ' + (a?.text || '—') + ' → ' + (b?.text || '—')));
    }
  } else d.append(E('p', c.text || c.reason || c.title || c.assessment?.reason || c.evidence?.note || 'Revision ' + c.revision + (c.run ? ' · run ' + c.run : '')));
  const raw = details(d, 'Event data');
  raw.append(E('pre', JSON.stringify(c, null, 2)));
}
function tasks(root, r) {
  const d = details(root, 'Tasks · ' + Object.keys(r.tasks).length);
  for (const [id, t] of Object.entries(r.tasks)) {
    d.append(E('p', id + ' · ' + t.title + ' · ' + t.acs.join(', ') + ' · ' + t.projects.join(', ') + ' · ' + (t.done ? 'Completed: ' + t.done : 'Assignee: ' + t.assignee)));
  }
}
function runs(root, r, base) {
  const d = details(root, 'Checks · ' + Object.keys(r.runs).length);
  for (const [id, run] of Object.entries(r.runs)) {
    const block = details(d, id + ' · ' + (run.finished ? 'Finished' : 'Running'));
    block.append(E('p', 'Subject: ' + JSON.stringify(run.subject)), E('p', 'Scope: ' + run.scope + ' · discovery ' + (run.discovery_complete ? 'complete' : 'incomplete')));
    for (const c of run.plan) {
      const result = run.results[c.id];
      const recorded = data.history.find(e => e.command.op === 'result' && e.command.req === base.req && e.command.revision === base.revision && e.command.run === id && e.command.evidence.check === c.id);
      if (recorded) block.append(E('small', recorded.actor + ' · ' + recorded.recorded_at));
      block.append(E('small','Method: '+c.method+' · AC: '+c.ac));
      if(result?.link)block.append(E('p','Evidence: '+result.link));
      block.append(E('h3', c.id), E('p', 'Expected: ' + c.expected.join(', ')), E('p', result ? 'Observed: ' + result.observed.join(', ') + ' · ' + result.outcome + ' · ' + result.note : 'No result yet'));
    }
  }
}
function comments(root, q, r) {
  const d = details(root, 'Discussion');
  d.open = true;
  for (const [id, c] of Object.entries(q.comments).filter(([, c]) => c.revision === r.number)) {
    d.append(E('small', id + ' · ' + c.actor + ' · ' + c.kind + (c.withdrawn ? ' · withdrawn' : '') + (c.reply_to ? ' · reply to ' + c.reply_to : '')), E('p', c.text));
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
function openRequirement(id) {
  filters={domain:'',status:'',person:''};render();
  const req=[...$('tree').querySelectorAll('.tree-requirement')].find(e=>e.dataset.requirement===id);
  if(!req)return;
  for(let node=req;node;node=node.parentElement)if(node.tagName==='DETAILS')node.open=true;
  req.scrollIntoView({block:'start'});
}
function intake(root) {
  const entries=Object.entries(data.state.intake);
  if(!entries.length)return;
  const section=details(root,'Intake · '+entries.length);
  for(const [id,item] of entries){
    const entry=details(section,id+' · '+item.author);entry.append(E('p',item.said));
    for(const [pid,point] of Object.entries(item.points)){
      const block=details(entry,pid+' · '+(point.outcome||'Unreviewed'));
      block.append(E('p',point.said));
      for(const [key,value] of Object.entries(point.answers||{}))block.append(E('p',key+': '+value));
      if(point.reason)block.append(E('p',point.reason));
      if(point.req)block.append(button(point.req,()=>openRequirement(point.req),'node-link'));
    }
  }
}
function copyButton(kind,id,title) {
  const b=button('',async()=>{
    const systemId=data.system_ids?.[kind]?.[id];
    if(!systemId)throw Error('System ID unavailable. Refresh the page.');
    const text=systemId+' — '+title;
    try{
      if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
      await navigator.clipboard.writeText(text);
      notify('Copied for your agent.');
    }catch{
      const dialog=E('dialog',undefined,'copy-dialog');
      dialog.setAttribute('aria-label','Copy for your agent');
      const area=E('input');area.readOnly=true;area.value=text;area.setAttribute('aria-label','ID and title');
      dialog.append(E('h2','Copy for your agent'),E('p','Automatic copying is unavailable. Select the text and copy it with ⌘C or Ctrl+C.'),area,button('Close',()=>dialog.close()));
      dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();area.focus();area.select();
    }
  },'copy-context');
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
  b.setAttribute('aria-label','Copy '+kind+' ID and title: '+id);
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}

function orderedEntries(kind,parent=null) {
  const map=kind==='domain'?data.state.domains:kind==='capability'?data.state.capabilities:data.state.requirements;
  const ids=Object.keys(map).filter(id=>kind==='domain'||(kind==='capability'?map[id].domain:map[id].capability)===parent).sort();
  const key=kind+(parent?':'+parent:'');
  const saved=(data.state.tree_order?.[key]||[]).filter(id=>ids.includes(id));
  return [...saved,...ids.filter(id=>!saved.includes(id))].map(id=>[id,map[id]]);
}
function appendNodeHistory(root,kind,id,q=null) {
  const history=details(root,'History');history.className='node-history';
  const created=data.history.find(e=>e.command.op===kind&&e.command.id===id)?.position||0;
  for(const event of data.history){
    if(event.position<created)continue;
    const c=event.command;
    const moved=c.op==='reorder_tree'&&c.kind===kind&&c.ids.includes(id)&&c.expected.indexOf(id)!==c.ids.indexOf(id);
    const changed=kind==='requirement'?c.req===id:((c.op===kind||c.op==='edit_'+kind)&&c.id===id)||c.op==='confirm_tree';
    if(moved||changed)historyEvent(history,event,q);
  }
}

function closeOrderMenu(){orderMenu?.remove();orderMenu=null;}
document.addEventListener('click',e=>{if(orderMenu&&!orderMenu.contains(e.target))closeOrderMenu();});
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeOrderMenu();});
async function saveOrder(kind,parent,expected,ids,focusId) {
  if(orderSaving||ids.every((id,index)=>id===expected[index]))return;
  const targetWorkspace=ws;orderSaving=true;closeOrderMenu();
  document.querySelectorAll('.reorder-handle').forEach(b=>b.disabled=true);
  try{
    const response=await fetch('/api/workspaces/'+encodeURIComponent(targetWorkspace)+'/order',{
      method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({request_id:crypto.randomUUID(),kind,parent,expected,ids})
    });
    const result=await response.json();
    if(!response.ok){if(ws===targetWorkspace)await load();throw Error(result.error||'Could not save the order.');}
    if(ws===targetWorkspace){
      await load();
    }
    notify('Order saved.');
  }finally{
    orderSaving=false;document.querySelectorAll('.reorder-handle').forEach(b=>b.disabled=false);
    if(ws===targetWorkspace){
      const moved=[...document.querySelectorAll('[data-tree-node]')].find(e=>e.dataset.treeNode===kind+':'+focusId);
      moved?.querySelector(':scope > summary > .reorder-handle')?.focus({preventScroll:true});
    }
  }
}
function moveSibling(kind,parent,id,offset) {
  const expected=orderedEntries(kind,parent).map(([id])=>id),ids=[...expected],index=ids.indexOf(id),next=index+offset;
  if(next<0||next>=ids.length)return;
  [ids[index],ids[next]]=[ids[next],ids[index]];
  return saveOrder(kind,parent,expected,ids,id);
}
function addOrdering(node,summary,kind,parent,id) {
  if(window.SNAPSHOT||viewerActor!==data.state.admin)return;
  const handle=E('button',undefined,'reorder-handle');
  const grip=document.createElementNS('http://www.w3.org/2000/svg','svg');grip.setAttribute('viewBox','0 0 16 20');grip.setAttribute('width','16');grip.setAttribute('height','20');grip.setAttribute('aria-hidden','true');
  for(const x of [5,11])for(const y of [4,10,16]){const dot=document.createElementNS(grip.namespaceURI,'circle');dot.setAttribute('cx',x);dot.setAttribute('cy',y);dot.setAttribute('r','1.4');dot.setAttribute('fill','currentColor');grip.append(dot);}handle.append(grip);handle.type='button';handle.draggable=true;handle.disabled=orderSaving;
  handle.setAttribute('aria-label','Reorder '+kind+': '+id);handle.setAttribute('aria-haspopup','menu');
  handle.title='Drag to reorder, or click for move options';summary.classList.add('can-reorder');summary.append(handle);
  handle.onclick=e=>{
    e.preventDefault();e.stopPropagation();closeOrderMenu();
    const menu=E('div',undefined,'reorder-menu');menu.setAttribute('role','menu');menu.setAttribute('aria-label','Move item');
    const siblings=orderedEntries(kind,parent),index=siblings.findIndex(([key])=>key===id);
    for(const [label,offset] of [['Move up',-1],['Move down',1]]){
      const b=button(label,()=>moveSibling(kind,parent,id,offset));b.setAttribute('role','menuitem');b.disabled=index+offset<0||index+offset>=siblings.length;menu.append(b);
    }
    orderMenu=menu;document.body.append(menu);
    const rect=handle.getBoundingClientRect();menu.style.left=Math.max(8,Math.min(rect.right-150,innerWidth-158))+'px';menu.style.top=Math.max(8,Math.min(rect.bottom+5,innerHeight-100))+'px';
    menu.querySelector('button:not(:disabled)')?.focus();
  };
  handle.onkeydown=e=>{if(['ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();e.stopPropagation();Promise.resolve(moveSibling(kind,parent,id,e.key==='ArrowUp'?-1:1)).catch(notify);}};
  const clear=()=>document.querySelectorAll('[data-drop]').forEach(e=>delete e.dataset.drop);
  const finish=()=>{draggedNode=null;clear();document.body.classList.remove('tree-dragging');node.classList.remove('drag-source');document.querySelectorAll('.drag-preview').forEach(e=>e.remove());};
  handle.ondragstart=e=>{
    if(orderSaving){e.preventDefault();return;}
    draggedNode={kind,parent,id,workspace:ws};e.dataTransfer.setData('text/plain',id);e.dataTransfer.effectAllowed='move';closeOrderMenu();closeFilterMenu();
    const preview=E('div',undefined,'drag-preview '+kind);preview.append(grip.cloneNode(true),E('div',undefined,'drag-preview-text'));
    preview.lastChild.append(E('small',kind==='requirement'?'Requirement':kind[0].toUpperCase()+kind.slice(1)),E('strong',summary.querySelector('.node-title').textContent));
    document.body.append(preview);e.dataTransfer.setDragImage(preview,28,28);
    requestAnimationFrame(()=>{if(draggedNode){node.classList.add('drag-source');document.body.classList.add('tree-dragging');}});
  };
  handle.ondragend=finish;
  summary.ondragover=e=>{
    clear();
    if(!draggedNode||orderSaving||draggedNode.workspace!==ws||draggedNode.kind!==kind||draggedNode.parent!==parent||draggedNode.id===id)return;
    e.preventDefault();e.stopPropagation();clear();e.dataTransfer.dropEffect='move';
    const rect=summary.getBoundingClientRect();summary.dataset.drop=e.clientY<rect.top+rect.height/2?'before':'after';
  };
  summary.ondragleave=e=>{if(!summary.contains(e.relatedTarget))delete summary.dataset.drop;};
  summary.ondrop=e=>{
    if(!summary.dataset.drop||!draggedNode||draggedNode.workspace!==ws||draggedNode.kind!==kind||draggedNode.parent!==parent)return;
    e.preventDefault();e.stopPropagation();
    const from=draggedNode.id,expected=orderedEntries(kind,parent).map(([id])=>id),ids=expected.filter(id=>id!==from);
    ids.splice(ids.indexOf(id)+(summary.dataset.drop==='after'?1:0),0,from);
    finish();saveOrder(kind,parent,expected,ids,from).catch(notify);
  };
}
