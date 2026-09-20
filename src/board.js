/* SNAPSHOT */
let ws = '', data = null;
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
let filters={domain:'',status:''};
let filterWorkspace=null;
function requirementStatus(id) {
  const r=data.state.requirements[id].revisions.at(-1),s=data.summary[id];
  if(Object.values(s.acs).some(a=>a.verification==='fail'))return 'fail';
  if(s.accepted)return 'pass';
  if(s.ready_for_acceptance)return 'ready';
  if(!r.confirmed||!r.acs.length||Object.values(s.acs).some(a=>!a.ready))return 'waiting';
  if(Object.keys(r.runs).length||Object.values(r.tasks).some(t=>t.purpose!=='decision'))return 'progress';
  return 'available';
}
function filterControls(root) {
  if(filterWorkspace!==ws){
    filterWorkspace=ws;filters={domain:'',status:''};
    try{const saved=JSON.parse(sessionStorage.getItem('joppa-filters:'+ws));
      if(saved && typeof saved.domain==='string' && typeof saved.status==='string')filters={domain:saved.domain,status:saved.status};
    }catch{}
  }
  if(filters.domain&&!data.state.domains[filters.domain])filters.domain='';
  if(filters.status&&!statusLabels[filters.status])filters.status='';
  const toolbar=E('section',undefined,'tree-toolbar');toolbar.setAttribute('aria-label','Filter product tree');
  const fields=E('div',undefined,'filter-fields');
  const domain=E('select');domain.id='domain-filter';domain.setAttribute('aria-label','Filter by domain');
  const all=E('option','All domains');all.value='';domain.append(all);
  for(const [id,d] of Object.entries(data.state.domains)){const option=E('option',d.title);option.value=id;domain.append(option);}
  domain.value=filters.domain;domain.onchange=()=>{filters.domain=domain.value;applyFilters();};
  fields.append(domain);toolbar.append(fields);
  const statuses=E('div',undefined,'status-filters');statuses.setAttribute('role','group');statuses.setAttribute('aria-label','Requirement status');
  for(const [key,label] of [['','All requirements'],...Object.entries(statusLabels)]){
    const b=button('',()=>{filters.status=filters.status===key?'':key;applyFilters();},'status-filter '+(key||'all'));
    b.dataset.status=key;b.append(E('span',label),E('strong','0','filter-count'));statuses.append(b);
  }
  toolbar.append(statuses);
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
        const match=domainAllowed;
        const status=requirementStatus(id);
        if(match)counts[status]++;
        row.hidden=!match||!!(filters.status&&filters.status!==status);
        if(!row.hidden){shown++;capVisible=true;if(reveal&&(filters.status||filters.domain))row.open=true;}
      }
      if(!filters.status&&domainAllowed)capVisible=true;
      cap.hidden=!capVisible;domainVisible ||= capVisible;
      if(capVisible&&reveal&&(filters.status||filters.domain))cap.open=true;
    }
    if(!filters.status&&domainAllowed)domainVisible=true;
    group.hidden=!domainVisible;
    if(domainVisible){visibleDomains++;if(reveal&&(filters.status||filters.domain))group.open=true;}
  }
  const total=Object.values(counts).reduce((a,b)=>a+b,0);
  for(const b of $('tree').querySelectorAll('.status-filter')){
    const key=b.dataset.status;b.setAttribute('aria-pressed',String(filters.status===key));
    b.querySelector('.filter-count').textContent=key?counts[key]:total;
  }
  $('filter-results').textContent=shown+' of '+Object.keys(data.state.requirements).length+' requirements · '+visibleDomains+' domains';
  $('filter-empty').hidden=visibleDomains>0||!Object.keys(data.state.domains).length;
}
function render() {
  const t = $('tree');
  const opened=new Set([...t.querySelectorAll('details[data-tree-node][open]')].map(d=>d.dataset.treeNode));
  t.replaceChildren();
  const intro=E('div',undefined,'tree-intro');
  intro.append(E('p','Product tree','tree-eyebrow'),E('h1',data.state.title));
  intro.append(E('p',`${Object.keys(data.state.domains).length} domains · ${Object.keys(data.state.capabilities).length} capabilities · ${Object.keys(data.state.requirements).length} requirements`,'node-stats'));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+data.launch.blockers.length,'hint'));
  t.append(intro);
  filterControls(t);
  for (const [did,d] of Object.entries(data.state.domains)) {
    const group=E('details',undefined,'tree-domain');group.dataset.domain=did;group.dataset.treeNode='domain:'+did;group.open=opened.has(group.dataset.treeNode);
    const heading=E('summary',undefined,'title-row');
    heading.append(E('span',d.title,'node-title'),E('small','DOMAIN','type-label'),E('span',d.confirmed?'Confirmed':'Draft','badge '+(d.confirmed?'confirmed':'waiting')),copyButton('domain',did,d.title));group.append(heading);
    const body=E('div',undefined,'domain-body');body.append(E('p','Decision owner: '+d.authority,'node-meta'));group.append(body);
    for(const [cid,c] of Object.entries(data.state.capabilities).filter(([,c])=>c.domain===did)){
      const cap=E('details',undefined,'tree-capability');cap.dataset.capability=cid;cap.dataset.treeNode='capability:'+cid;cap.open=opened.has(cap.dataset.treeNode);
      const head=E('summary',undefined,'title-row');head.append(E('span',c.title,'node-title'),E('small','CAPABILITY','type-label'),copyButton('capability',cid,c.title));cap.append(head);
      const reqs=Object.entries(data.state.requirements).filter(([,q])=>q.capability===cid);
      if(!reqs.length)cap.append(E('p','No requirements yet.','empty-state'));
      for(const [id,q] of reqs){
        const r=q.revisions.at(-1),tone=requirementStatus(id);
        const req=E('details',undefined,'tree-requirement '+tone);req.dataset.requirement=id;req.dataset.treeNode='requirement:'+id;req.open=opened.has(req.dataset.treeNode);
        const reqHead=E('summary',undefined,'title-row');reqHead.append(E('span',r.title,'node-title'),E('small','REQ','type-label'),E('span',statusLabels[tone],'badge '+tone),copyButton('requirement',id,r.title));
        const content=E('div',undefined,'requirement-detail');req.append(reqHead,content);
        req.addEventListener('toggle',()=>{if(req.open&&!content.childNodes.length)requirementDetails(content,id);});
        if(req.open)requirementDetails(content,id);
        cap.append(req);
      }
      body.append(cap);
    }
    t.append(group);
  }
  if(!Object.keys(data.state.domains).length)t.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
  const empty=E('p','No requirements match this status.','filter-empty');empty.id='filter-empty';empty.hidden=true;t.append(empty);
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
    item.append(E('h3', a.text), E('small','AC · '+a.id), E('span', status?.verification || 'See history', 'badge ' + (status?.verification || '')), E('p', status?.ready ? 'Ready to work' : a.assessment?.reason || 'Awaiting review'));
    if (a.before_launch) item.append(E('small', 'Required before launch'));
    if (a.depends_on?.length) item.append(E('p','Depends on: '+a.depends_on.join(', ')));
    if (a.decider) item.append(E('p', 'Decision: ' + a.decider));
    root.append(item);
  }
  tasks(root, r);
  runs(root, r, base);
  comments(root, q, r);
  const h = details(root, 'Revision history');
  for (const ev of data.history.filter(e => e.command.req === selected)) {
    historyEvent(h, ev, q);
  }
}
function historyEvent(parent, event, q) {
  const names = {
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
  if (c.op === 'revise') {
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
  filters={domain:'',status:''};render();
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
    const text=id+' — '+title;
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
  icon.append(square,back);b.append(icon,E('span','Agent'));
  b.title='Copy ID and title for your agent';
  b.setAttribute('aria-label','Copy '+kind+' ID and title: '+id);
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}
