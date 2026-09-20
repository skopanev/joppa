/* SNAPSHOT */
let actor = '',
  ws = '',
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
async function api(path, body) {
  const r = await fetch('/api/' + path, {
    method: body ? 'POST' : 'GET',
    headers: {
      ...(body ? {
        'Content-Type': 'application/json'
      } : {})
    },
    body: body ? JSON.stringify(body) : undefined
  });
  const j = await r.json();
  if(r.status===401){loadNumber++;data=null;actor='';selected='';selectedNode=null;document.body.classList.remove('authenticated','detail-open');$('tree').replaceChildren();$('card').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')){data=null;selected='';selectedNode=null;document.body.classList.remove('detail-open');$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));$('card').replaceChildren();}
  if (!r.ok) throw Error(j.error || JSON.stringify(j));
  return j;
}
// getRandomValues also works on the private-network HTTP address.
function newId() {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  const hex = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
}
async function send(command) {
  await api('workspaces/' + encodeURIComponent(ws) + '/commands', {
    request_id: newId(),
    command
  });
  await load();
  notify('Saved to history.');
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
function field(form, name, label, type = 'text', value = '', options = null) {
  const l = E('label', label);
  let i;
  if (options) {
    i = E('select');
    for (const [v, t] of options) {
      const o = E('option', t);
      o.value = v;
      i.append(o);
    }
  } else i = E(type === 'textarea' ? 'textarea' : 'input');
  i.name = name;
  i.setAttribute('aria-label', label);
  if (i.tagName === 'INPUT') i.type = type;
  if (type === 'checkbox') i.checked = !!value;else i.value = value ?? '';
  l.append(i);
  const submit = form.querySelector(':scope > button[type=submit]');
  if (submit) form.insertBefore(l, submit);else form.append(l);
  return i;
}
function form(parent, label, fn) {
  const f = E('form');
  const b = E('button', label);
  b.type = 'submit';
  f.append(b);
  f.onsubmit = async e => {
    e.preventDefault();
    b.disabled = true;
    notify('');
    try {
      await fn(new FormData(f), f);
    } catch (e) {
      notify(e);
    } finally {
      b.disabled = false;
    }
  };
  parent.append(f);
  return f;
}
const members = () => Object.entries(data.state.members).map(([id, role]) => [id, id + ' · ' + role]);
const humans = () => members().filter(([id]) => data.state.members[id] === 'human');
const options = map => Object.entries(map).map(([id, v]) => [id, v.title || id]);
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
  bootstrap();
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
  actor=identity.actor;
  $('identity').textContent=identity.email||actor;
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
function bootstrap() {
  const s = $('setup');
  s.replaceChildren();
  const d = details(s, 'New workspace');
  const f = form(d, 'Create', async v => {
    ws = v.get('id');
    await send({
      op: 'workspace',
      title: v.get('title')
    });
    await list();
  });
  field(f, 'id', 'Workspace ID');
  field(f, 'title', 'Name');
}
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
  if(!Object.keys(data.state.domains).length)t.append(E('p','No domains yet. Add the product structure through MCP or workspace settings.','empty-state'));
  if (!window.SNAPSHOT) {
    const actions=E('div',undefined,'tree-actions');settings(actions);intake(actions);t.append(actions);
  }
  if (selected && !data.state.requirements[selected]) selected = '';
  card();
}
function settings(t) {
  const d = details(t, 'Workspace settings');
  let f = form(d, 'Add member', v => send({
    op: 'member',
    id: v.get('id'),
    role: v.get('role')
  }));
  field(f, 'id', 'Member ID');
  field(f, 'role', 'Role', 'text', 'human', [['human', 'Human'], ['agent', 'Agent'], ['runner', 'CI']]);
  f = form(d, 'Add domain', v => send({
    op: 'domain',
    id: v.get('id'),
    title: v.get('title'),
    authority: v.get('authority')
  }));
  field(f, 'id', 'Domain ID');
  field(f, 'title', 'Name');
  field(f, 'authority', 'Decision owner', 'text', '', humans());
  f = form(d, 'Add capability', v => send({
    op: 'capability',
    id: v.get('id'),
    title: v.get('title'),
    domain: v.get('domain')
  }));
  field(f, 'id', 'Capability ID');
  field(f, 'title', 'We can…');
  field(f, 'domain', 'Domain', 'text', '', options(data.state.domains));
  d.append(button('Confirm structure', () => send({
    op: 'confirm_tree'
  })));
  f = form(d, 'Add repository', v => send({
    op: 'project',
    id: v.get('id'),
    repository: v.get('repository')
  }));
  field(f, 'id', 'Project ID');
  field(f, 'repository', 'Repository URL');
}
function intake(t) {
  const d = details(t, 'Intake · ' + Object.keys(data.state.intake).length);
  const f = form(d, 'Save original wording', v => send({
    op: 'intake',
    id: 'i-' + newId(),
    said: v.get('said'),
    author: v.get('author')
  }));
  field(f, 'author', 'Author', 'text', actor);
  field(f, 'said', 'Request', 'textarea');
  for (const [id, i] of Object.entries(data.state.intake)) {
    const item = details(d, id + ' · ' + i.author);
    item.append(E('p', i.said));
    if (!Object.keys(i.points).length) {
      const split = form(item, 'Split into items', v => send({
        op: 'split',
        id,
        points: v.get('points').split('\n').filter(Boolean)
      }));
      field(split, 'points', 'One item per line', 'textarea', i.said);
    }
    for (const [pid, p] of Object.entries(i.points)) {
      const point = details(item, pid + ' · ' + (p.outcome || 'Unreviewed'));
      point.append(E('p', p.said));
      if (p.outcome) {
        point.append(E('p', p.reason));
        continue;
      }
      let f = form(point, 'Assign category', v => send({
        op: 'sort',
        id,
        point: pid,
        kind: v.get('kind')
      }));
      field(f, 'kind', 'Category', 'text', p.kind, Object.keys(data.state.questions).map(k => [k, k]));
      for (const key of data.state.questions[p.kind] || []) {
        f = form(point, 'Save answer', v => send({
          op: 'answer',
          id,
          point: pid,
          field: key,
          value: v.get('value')
        }));
        field(f, 'value', key, 'textarea', p.answers[key] || '');
      }
      f = form(point, 'Record decision', v => send({
        op: 'route',
        id,
        point: pid,
        outcome: v.get('outcome'),
        reason: v.get('reason'),
        req: v.get('outcome') === 'work' ? v.get('req') : null
      }));
      field(f, 'outcome', 'Decision', 'text', 'work', [['work', 'Proceed'], ['refused', 'Decline'], ['elsewhere', 'Handle elsewhere']]);
      field(f, 'req', 'Requirement', 'text', '', Object.keys(data.state.requirements).map(k => [k, k]));
      field(f, 'reason', 'Reason', 'textarea');
    }
  }
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
  const heading=E('div',undefined,'title-row');heading.append(E('h1',r.title),E('small','REQ','type-label'));root.append(heading,E('p',selected+' · revision '+r.number+' · owner '+r.owner),E('p',r.body));
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
  if (current && !window.SNAPSHOT) {
    root.append(button('Edit requirement', () => edit(r), 'write'), button('Confirm wording', () => send({
      ...base,
      op: 'confirm'
    }), 'write'));
    if (s.ready_for_acceptance && !s.accepted) root.append(button('Accept result', () => send({
      ...base,
      op: 'accept',
      run: s.run
    }), 'write'));
  }
  for (const a of r.acs) {
    const status = s?.acs[a.id];
    const item = E('article',undefined,'ac-card '+(status?.verification==='pass'?'pass':status?.verification==='fail'?'fail':'unverified'));
    item.append(E('h3', a.text), E('small','AC · '+a.id), E('span', status?.verification || 'See history', 'badge ' + (status?.verification || '')), E('p', status?.ready ? 'Ready to work' : a.assessment?.reason || 'Awaiting review'));
    if (a.before_launch) item.append(E('small', 'Required before launch'));
    if (a.decider) item.append(E('p', 'Decision: ' + a.decider));
    if (current && !window.SNAPSHOT) {
      const d = details(item, 'Assess readiness');
      const f = form(d, 'Save assessment', v => send({
        ...base,
        op: 'assess',
        ac: a.id,
        assessment: {
          ready: v.get('ready') === 'yes',
          reason: v.get('reason'),
          recipient: v.get('recipient') || null,
          decision: v.get('decision') || null
        }
      }));
      field(f, 'ready', 'Readiness', 'text', 'no', [['no', 'Awaiting decision'], ['yes', 'Ready to work']]);
      field(f, 'reason', 'Reason or specific question', 'textarea', a.assessment?.reason || '');
      field(f, 'recipient', 'Question for', 'text', '', [['', '—'], ...members()]);
      field(f, 'decision', 'Decision in discussion', 'text', '', [['', '—'], ...Object.entries(q.comments).filter(([, c]) => c.kind === 'decision' && !c.withdrawn).map(([id, c]) => [id, c.actor + ': ' + c.text.slice(0, 70)])]);
    }
    root.append(item);
  }
  tasks(root, r, base, current);
  runs(root, r, base, current);
  comments(root, q, r, base);
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
function edit(r) {
  if(!r)return;
  selectedNode = null;
  document.body.classList.add('detail-open');
  const root = $('card');
  root.replaceChildren(E('h2','New revision · all checks reset to n/a'));
  const f = form(root, 'Save revision', v => {
    const acs = [...f.querySelectorAll('fieldset')].map(row => ({
      id: row.querySelector('[name=ac_id]').value,
      text: row.querySelector('[name=ac_text]').value,
      decider: row.querySelector('[name=decider]').value || null,
      before_launch: row.querySelector('[name=before_launch]').checked,
      depends_on: row.querySelector('[name=depends]').value.split(',').map(s => s.trim()).filter(Boolean)
    }));
    selected = v.get('req');
    revisionNumber = null;
    return send({
      op: 'revise',
      req: selected,
      expected_revision: r.number,
      capability: v.get('capability'),
      title: v.get('title'),
      body: v.get('body'),
      owner: v.get('owner'),
      reason: v.get('reason'),
      acs
    });
  });
  field(f, 'req', 'Requirement ID', 'text', selected).readOnly = true;
  field(f, 'capability', 'Capability', 'text', data.state.requirements[selected].capability, options(data.state.capabilities));
  field(f, 'title', 'What must be true', 'text', r?.title);
  field(f, 'body', 'Context', 'textarea', r?.body);
  field(f, 'owner', 'Owner', 'text', r?.owner || actor, humans());
  field(f, 'reason', 'Reason for change', 'textarea');
  function acrow(a = {}) {
    const row = E('fieldset');
    field(row, 'ac_id', 'Stable AC ID', 'text', a.id || '');
    field(row, 'ac_text', 'Acceptance criterion, including the relevant platform', 'textarea', a.text || '');
    field(row, 'decider', 'Decision needed from', 'text', a.decider || '', [['', 'Not required'], ...humans()]);
    field(row, 'depends', 'Depends on ACs (comma-separated IDs)', 'text', (a.depends_on || []).join(', '));
    field(row, 'before_launch', 'Required before launch', 'checkbox', a.before_launch);
    row.append(button('Remove from this revision', () => row.remove()));
    f.insertBefore(row, f.querySelector('button[type=submit]'));
  }
  for (const a of r?.acs || [{}]) acrow(a);
  f.append(button('Add AC', () => acrow()));
  root.append(button('Cancel', card));
}
function tasks(root, r, base, current) {
  const d = details(root, 'Tasks · ' + Object.keys(r.tasks).length);
  for (const [id, t] of Object.entries(r.tasks)) {
    d.append(E('p', id + ' · ' + t.title + ' · ' + t.acs.join(', ') + ' · ' + t.projects.join(', ') + ' · ' + (t.done ? 'Completed: ' + t.done : 'Assignee: ' + t.assignee)));
    if (current && !t.done && !window.SNAPSHOT) {
      const f = form(d, 'Complete task', v => send({
        ...base,
        op: 'complete_task',
        id,
        evidence: v.get('evidence')
      }));
      field(f, 'evidence', t.purpose === 'decision' ? 'Decision ID from discussion' : 'What changed and where to find the result', 'textarea');
    }
  }
  if (!current || window.SNAPSHOT) return;
  const f = form(d, 'Create task', v => send({
    ...base,
    op: 'task',
    id: v.get('id'),
    title: v.get('title'),
    acs: v.getAll('acs'),
    projects: v.getAll('projects'),
    purpose: v.get('purpose'),
    assignee: v.get('assignee')
  }));
  field(f, 'id', 'Task ID');
  field(f, 'title', 'What to do');
  field(f, 'assignee', 'Assignee', 'text', actor, members());
  field(f, 'purpose', 'Work type', 'text', 'implementation', [['implementation', 'Implementation'], ['manual', 'Configuration / manual work'], ['decision', 'Get a decision']]);
  const a = field(f, 'acs', 'Acceptance criteria', 'text', '', r.acs.map(a => [a.id, a.id]));
  a.multiple = true;
  const p = field(f, 'projects', 'Repositories (optional)', 'text', '', options(data.state.projects));
  p.multiple = true;
}
function runs(root, r, base, current) {
  const d = details(root, 'Checks · ' + Object.keys(r.runs).length);
  for (const [id, run] of Object.entries(r.runs)) {
    const block = details(d, id + ' · ' + (run.finished ? 'Finished' : 'Running'));
    block.append(E('p', 'Subject: ' + JSON.stringify(run.subject)), E('p', 'Scope: ' + run.scope + ' · discovery ' + (run.discovery_complete ? 'complete' : 'incomplete')));
    for (const c of run.plan) {
      const result = run.results[c.id];
      const recorded = data.history.find(e => e.command.op === 'result' && e.command.req === base.req && e.command.revision === base.revision && e.command.run === id && e.command.evidence.check === c.id);
      if (recorded) block.append(E('small', recorded.actor + ' · ' + recorded.recorded_at));
      block.append(E('h3', c.id), E('p', 'Expected: ' + c.expected.join(', ')), E('p', result ? 'Observed: ' + result.observed.join(', ') + ' · ' + result.outcome + ' · ' + result.note : 'No result yet'));
      if (!run.finished && !result && c.method === 'manual' && !window.SNAPSHOT) {
        const f = form(block, 'Record check', v => send({
          ...base,
          op: 'result',
          run: id,
          evidence: {
            check: c.id,
            outcome: v.get('outcome'),
            observed: v.get('observed').split('\n').filter(Boolean),
            note: v.get('note'),
            link: v.get('link') || null
          }
        }));
        field(f, 'outcome', 'Result', 'text', 'pass', [['pass', 'Passed'], ['fail', 'Failed'], ['skip', 'Skipped']]);
        field(f, 'observed', 'Observed list — one item per line', 'textarea');
        field(f, 'note', 'What was checked and how', 'textarea');
        field(f, 'link', 'Evidence URL');
      }
    }
    if (!run.finished && !window.SNAPSHOT) block.append(button('Finish run', () => send({
      ...base,
      op: 'finish',
      run: id
    })));
  }
  if (current && !window.SNAPSHOT) {
    const f = form(d, 'Seal manual check plan', v => send({
      ...base,
      op: 'plan',
      run: v.get('id'),
      subject: {
        environment: v.get('subject')
      },
      scope: v.get('scope'),
      discovery_complete: v.get('complete') === 'on',
      checks: r.acs.map(a => ({
        id: 'manual-' + a.id,
        ac: a.id,
        method: 'manual',
        expected: v.get('expected-' + a.id).split('\n').filter(Boolean)
      }))
    }));
    field(f, 'id', 'Run ID');
    field(f, 'subject', 'Environment and configuration version');
    field(f, 'scope', 'Check scope');
    field(f, 'complete', 'All checks in this scope are included', 'checkbox');
    for (const a of r.acs) field(f, 'expected-' + a.id, a.id + ' · expected list', 'textarea');
  }
}
function comments(root, q, r, base) {
  const d = details(root, 'Discussion');
  d.open = true;
  for (const [id, c] of Object.entries(q.comments).filter(([, c]) => c.revision === r.number)) {
    d.append(E('small', id + ' · ' + c.actor + ' · ' + c.kind + (c.withdrawn ? ' · withdrawn' : '') + (c.reply_to ? ' · reply to ' + c.reply_to : '')), E('p', c.text));
  }
  if (window.SNAPSHOT) return;
  const f = form(d, 'Post reply', v => send({
    ...base,
    op: 'comment',
    id: 'c-' + newId(),
    text: v.get('text'),
    kind: v.get('kind'),
    reply_to: v.get('reply_to') || null,
    ac: v.get('ac') || null
  }));
  field(f, 'kind', 'Reply type', 'text', 'comment', [['comment', 'Comment / suggestion'], ['decision', 'Specialist decision'], ['accept', 'Accept suggestion'], ['change', 'Amend suggestion'], ['reject', 'Reject with reason'], ['correction', 'Correct earlier reply']]);
  field(f, 'text', 'Reply', 'textarea');
  field(f, 'reply_to', 'Reply to comment ID');
  field(f, 'ac', 'Acceptance criterion', 'text', '', [['', 'Entire requirement'], ...r.acs.map(a => [a.id, a.id])]);
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
  const heading=E('div',undefined,'title-row');heading.append(E('h1',node.title),E('small',kind==='domain'?'DOMAIN':'CAPABILITY','type-label'));root.append(heading);
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
  const help=details(root,kind==='domain'?'What is a domain?':'What is a capability?');help.classList.add('node-help');
  help.append(E('p',kind==='domain'?'A domain is a business area with a clear decision owner.':'A capability describes what the product can do. Start with “We can…”','hint'),E('p','Domains and capabilities organize the work. Checks verify the acceptance criteria of individual requirements.','hint'));
  if(!window.SNAPSHOT&&actor===data.state.admin){
    const edit=details(root,kind==='domain'?'Edit domain':'Edit capability');
    const f=form(edit,'Save changes',v=>send({op:kind==='domain'?'edit_domain':'edit_capability',id,title:v.get('title'),reason:v.get('reason'),...(kind==='domain'?{authority:v.get('authority')}:{})}));
    field(f,'title','Name','text',node.title);if(kind==='domain')field(f,'authority','Decision owner','text',domain.authority,humans());field(f,'reason','Reason for change','textarea');
  }
}
