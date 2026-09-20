/* SNAPSHOT */
let ws = '',
  data = null,
  selected = '',
  revisionNumber = null,
  selectedNode = null;
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
  if(r.status===401){loadNumber++;data=null;selected='';selectedNode=null;document.body.classList.remove('authenticated','detail-open');$('tree').replaceChildren();$('card').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')){data=null;selected='';selectedNode=null;document.body.classList.remove('detail-open');$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));$('card').replaceChildren();}
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
  else {document.body.classList.remove('detail-open');data=null;$('tree').replaceChildren(E('h1','No workspace access'),E('p','Ask your workspace owner to grant access.'));$('card').replaceChildren();}
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
  ws=e.target.value;sessionStorage.setItem('joppa-workspace',ws);selectedNode=null;selected='';revisionNumber=null;
  $('card').replaceChildren(E('p','Opening workspace…'));$('tree').replaceChildren();
  try{await load();}catch(e){notify(e);}
};
$('mobile-tree').onclick=()=>{selectedNode=null;selected='';revisionNumber=null;card();};
$('refresh').onclick = () => load().catch(notify);
function render() {
  const t = $('tree');
  const collapsed=new Set([...t.querySelectorAll('.tree-domain:not([open])')].map(d=>d.dataset.domain));
  t.replaceChildren();
  const intro=E('div',undefined,'tree-intro');
  intro.append(E('p','Product tree','tree-eyebrow'),E('h1',data.state.title));
  intro.append(E('p',`${Object.keys(data.state.domains).length} domains · ${Object.keys(data.state.capabilities).length} capabilities · ${Object.keys(data.state.requirements).length} requirements`,'node-stats'));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+data.launch.blockers.length,'hint'));
  t.append(intro);
  for (const [did, d] of Object.entries(data.state.domains)) {
    const group = E('details',undefined,'tree-domain');
    group.dataset.domain=did;group.open=!collapsed.has(did);
    const domainHeading=E('summary',undefined,'title-row');
    const domainButton=button(d.title,()=>openNode('domain',did),'node-link');
    domainButton.addEventListener('click',e=>e.preventDefault());
    domainHeading.append(domainButton,E('small','DOMAIN','type-label'),E('span',d.confirmed?'Confirmed':'Draft','badge '+(d.confirmed?'confirmed':'waiting')));
    domainHeading.dataset.node='domain:'+did;group.append(domainHeading);
    const domainBody=E('div',undefined,'domain-body');group.append(domainBody);
    for (const [cid, c] of Object.entries(data.state.capabilities).filter(([, c]) => c.domain === did)) {
      const capability=E('section',undefined,'tree-capability');
      const capabilityHeading=E('div',undefined,'title-row');capabilityHeading.dataset.node='capability:'+cid;capabilityHeading.append(button(c.title,()=>openNode('capability',cid),'node-link cap-link'),E('small','CAPABILITY','type-label'));capability.append(capabilityHeading);
      for (const [id, q] of Object.entries(data.state.requirements).filter(([, q]) => q.capability === cid)) {
        const r = q.revisions.at(-1),
          s = data.summary[id];
        const failed=Object.values(s.acs).some(a=>a.verification==='fail');
        const tone=failed?'fail':s.accepted?'pass':s.ready_for_acceptance?'ready':'waiting';
        const label=failed?'Checks failed':s.accepted?'Accepted':s.ready_for_acceptance?'Awaiting acceptance':r.confirmed?'In progress':'Awaiting confirmation';
        const req=E('div',undefined,'tree-requirement '+tone);
        const reqHeading=E('div',undefined,'title-row');
        const reqButton=button(r.title,()=>openRequirement(id),'node-link tree-req');reqButton.dataset.node='req:'+id;
        reqHeading.append(reqButton,E('small','REQ','type-label'));req.append(reqHeading,E('span',label,'badge '+tone));
        const acs=E('ul',undefined,'tree-acs');
        for(const ac of r.acs){
          const status=s.acs[ac.id], row=E('li');
          const verdict=status?.verification||'n/a';
          row.append(E('span',ac.text,'ac-text'),E('small','AC','type-label'),E('span',verdict,'badge '+(verdict==='pass'?'pass':verdict==='fail'?'fail':'unverified')));
          acs.append(row);
        }
        req.append(acs);capability.append(req);
      }
      domainBody.append(capability);
    }
    t.append(group);
  }
  if(!Object.keys(data.state.domains).length)t.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
  intake(t);
  if (selected && !data.state.requirements[selected]) selected = '';
  card();
}
function card() {
  const root = $('card');
  document.body.classList.toggle('detail-open',!!(selectedNode||selected));
  refreshSelection();
  root.replaceChildren();
  if (selectedNode) {nodeCard(root);return;}
  if (!selected) return;
  const q = data.state.requirements[selected],
    r = q.revisions.find(r => r.number === revisionNumber) || q.revisions.at(-1),
    current = r === q.revisions.at(-1),
    s = current ? data.summary[selected] : null;
  const heading=E('div',undefined,'title-row');heading.append(E('h1',r.title),E('small','REQ','type-label'));heading.append(copyButton('requirement',selected,r.number));root.append(heading,E('p',selected+' · revision '+r.number+' · owner '+r.owner),E('p',r.body));
  const revisions = E('select');
  for (const old of q.revisions) {
    const o = E('option', 'Revision ' + old.number + ' · ' + old.reason);
    o.value = old.number;
    revisions.append(o);
  }
  revisions.value = r.number;
  revisions.onchange = e => {
    revisionNumber = +e.target.value;
    card();
  };
  root.append(revisions);
  root.append(E('p', s ? s.accepted ? 'Accepted' : s.ready_for_acceptance ? 'Ready for acceptance' : r.confirmed ? 'In progress' : 'Wording awaiting confirmation' : 'Previous revision. See its checks and history for evidence.'));
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
  fetch('/api/me').then(async r=>{if(r.ok)await showApp(await r.json());}).catch(notify);
}
function closeMobileTree() {
  document.body.classList.add('detail-open');
  window.scrollTo(0,0);
}
function refreshSelection() {
  const key=selectedNode?selectedNode.kind+':'+selectedNode.id:'req:'+selected;
  for(const row of document.querySelectorAll('[data-node]')) {
    const active=row.dataset.node===key;row.classList.toggle('active',active);
    const b=row.matches('button')?row:row.querySelector('button');if(b){if(active)b.setAttribute('aria-current','page');else b.removeAttribute('aria-current');}
  }
}
function openNode(kind,id) {selected='';revisionNumber=null;selectedNode={kind,id};closeMobileTree();card();}
function openRequirement(id) {selectedNode=null;selected=id;revisionNumber=null;closeMobileTree();card();}
function sectionHead(root,title,count) {const head=E('div',undefined,'node-section-head');head.append(E('h2',title),E('small',String(count)));root.append(head);}
function catalogRow(root,title,kind,action,count) {
  const row=E('div',undefined,'catalog-row');row.append(button(title,action,'node-link'),E('small',kind,'type-label'));
  if(count!==undefined){const counts=E('div',undefined,'counts');counts.append(E('span',count+' REQ'));row.append(counts);}
  root.append(row);return row;
}
function nodeCard(root) {
  const {kind,id}=selectedNode;
  const domain=kind==='domain'?data.state.domains[id]:data.state.domains[data.state.capabilities[id].domain];
  const node=kind==='domain'?domain:data.state.capabilities[id];
  root.append(button('← '+data.state.title,()=>{selectedNode=null;card();},'back-link'));
  const heading=E('div',undefined,'title-row');heading.append(E('h1',node.title),E('small',kind==='domain'?'DOMAIN':'CAPABILITY','type-label'));heading.append(copyButton(kind,id));root.append(heading);
  const meta=E('div',undefined,'node-meta');meta.append(E('span',node.confirmed?'Confirmed':'Draft','badge'),E('span',kind==='domain'?'Decision owner: '+domain.authority:domain.title));root.append(meta);
  if(kind==='domain') {
    const caps=Object.entries(data.state.capabilities).filter(([,c])=>c.domain===id);
    sectionHead(root,'Capabilities',caps.length);
    for(const [cid,c]of caps){const count=Object.values(data.state.requirements).filter(q=>q.capability===cid).length;catalogRow(root,c.title,'CAPABILITY',()=>openNode('capability',cid),count);}
  } else {
    const reqs=Object.entries(data.state.requirements).filter(([,q])=>q.capability===id);
    sectionHead(root,'Requirements',reqs.length);
    for(const [rid,q]of reqs)catalogRow(root,q.revisions.at(-1).title,'REQ',()=>openRequirement(rid));
    if(!reqs.length){const empty=E('div',undefined,'empty-state');empty.append(E('p','No requirements here yet.'));root.append(empty);}
  }
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
function agentContext(kind,id,number) {
  const state=data.state;
  const requirement=kind==='requirement'?state.requirements[id]:null;
  const capability=kind==='capability'?state.capabilities[id]:requirement?state.capabilities[requirement.capability]:null;
  const domainId=kind==='domain'?id:capability.domain;
  const capabilities=Object.fromEntries(Object.entries(state.capabilities).filter(([cid,c])=>kind==='domain'?c.domain===id:cid===(requirement?requirement.capability:id)));
  const requirements=Object.fromEntries(Object.entries(state.requirements).filter(([rid,q])=>kind==='requirement'?rid===id:Object.hasOwn(capabilities,q.capability)));
  const summary=Object.fromEntries(Object.entries(data.summary).filter(([rid])=>Object.hasOwn(requirements,rid)));
  const history=data.history.filter(({command:c})=>
    (c.req && Object.hasOwn(requirements,c.req)) ||
    (['domain','edit_domain'].includes(c.op) && c.id===domainId) ||
    (['capability','edit_capability'].includes(c.op) && Object.hasOwn(capabilities,c.id)) ||
    ['workspace','rename_workspace','confirm_tree'].includes(c.op));
  const context={workspace:{id:ws,title:state.title},position:data.position,
    selection:{kind,id,...(number?{revision:number}:{})},
    domain:{id:domainId,...state.domains[domainId]},capabilities,requirements,
    latest_revision_summary:summary,history};
  return 'Joppa — '+kind+' context\n'+
    'Discuss this item with me. Read the latest workspace through Joppa MCP before proposing changes. '+
    'Preserve the stated scope; do not add platforms or acceptance criteria by assumption.\n'+
    'This is a snapshot, not live state. The selection identifies the revision being viewed; summaries describe the latest revisions.\n\n'+
    JSON.stringify(context,null,2);
}
function copyButton(kind,id,number) {
  const b=button('Copy for agent',async()=>{
    const text=agentContext(kind,id,number);
    try{
      if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
      await navigator.clipboard.writeText(text);
      notify('Context copied. Paste it into your agent chat.');
    }catch{
      const dialog=E('dialog',undefined,'copy-dialog');
      dialog.setAttribute('aria-label','Copy context for your agent');
      const area=E('textarea');area.readOnly=true;area.value=text;area.setAttribute('aria-label','Agent context');
      dialog.append(E('h2','Copy for agent'),E('p','Automatic copying is unavailable. Select the text and copy it with ⌘C or Ctrl+C.'),area,button('Close',()=>dialog.close()));
      dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();area.focus();area.select();
    }
  },'copy-context');
  b.title='Copy this '+kind+' and its context for an agent';
  return b;
}
