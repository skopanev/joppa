/* SNAPSHOT */
let token = '',
  actor = '',
  ws = '',
  data = null,
  selected = '',
  revisionNumber = null;
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
const notify = e => $('notice').textContent = e?.message || String(e || '');
async function api(path, body) {
  const r = await fetch('/api/' + path, {
    method: body ? 'POST' : 'GET',
    headers: {
      Authorization: 'Bearer ' + token,
      ...(body ? {
        'Content-Type': 'application/json'
      } : {})
    },
    body: body ? JSON.stringify(body) : undefined
  });
  const j = await r.json();
  if (!r.ok) throw Error(j.error || JSON.stringify(j));
  return j;
}
async function send(command) {
  await api('workspaces/' + encodeURIComponent(ws) + '/commands', {
    request_id: crypto.randomUUID(),
    command
  });
  await load();
  notify('Сохранено. История дополнена.');
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
  for (const w of list) {
    const o = E('option', w.title);
    o.value = w.id;
    $('workspaces').append(o);
  }
  if (!list.some(w => w.id === ws)) ws = list[0]?.id || '';
  $('workspaces').value = ws;
  bootstrap();
  if (ws) await load();
}
let loadNumber = 0;
async function load() {
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)));
  if (request !== loadNumber) return;
  data = next;
  render();
}
$('login').onsubmit = async e => {
  e.preventDefault();
  token = new FormData(e.target).get('token');
  try {
    actor = (await api('me')).actor;
    $('identity').textContent = actor;
    await list();
  } catch (e) {
    notify(e);
  }
};
$('workspaces').onchange = async e => {
  ws = e.target.value;
  selected = '';
  revisionNumber = null;
  try {
    await load();
  } catch (e) {
    notify(e);
  }
};
$('refresh').onclick = () => load().catch(notify);
function bootstrap() {
  const s = $('setup');
  s.replaceChildren();
  const d = details(s, 'Создать продукт');
  const f = form(d, 'Создать', async v => {
    ws = v.get('id');
    await send({
      op: 'workspace',
      title: v.get('title')
    });
    await list();
  });
  field(f, 'id', 'Короткий ID продукта');
  field(f, 'title', 'Название');
}
function render() {
  const t = $('tree');
  t.replaceChildren();
  t.append(E('h2', data.state.title));
  t.append(E('p', !data.launch.configured ? 'Условия запуска ещё не заданы' : data.launch.ready ? 'Обязательные условия запуска приняты' : 'До запуска осталось условий: ' + data.launch.blockers.length));
  for (const [did, d] of Object.entries(data.state.domains)) {
    const group = E('section');
    group.append(E('h3', d.title), E('small', 'Решает: ' + d.authority + (d.confirmed ? '' : ' · ждёт подтверждения')));
    for (const [cid, c] of Object.entries(data.state.capabilities).filter(([, c]) => c.domain === did)) {
      group.append(E('p', c.title));
      for (const [id, q] of Object.entries(data.state.requirements).filter(([, q]) => q.capability === cid)) {
        const r = q.revisions.at(-1),
          s = data.summary[id];
        group.append(button((s.accepted ? '✓ ' : s.ready_for_acceptance ? '◉ ' : '○ ') + r.title, () => {
          selected = id;
          revisionNumber = null;
          card();
        }, 'nav'));
      }
    }
    t.append(group);
  }
  if (!window.SNAPSHOT) {
    t.append(button('Новое требование', () => edit(null), 'write'));
    settings(t);
    intake(t);
  }
  if (!selected || !data.state.requirements[selected]) selected = Object.keys(data.state.requirements)[0] || '';
  card();
}
function settings(t) {
  const d = details(t, 'Настроить продукт');
  let f = form(d, 'Добавить участника', v => send({
    op: 'member',
    id: v.get('id'),
    role: v.get('role')
  }));
  field(f, 'id', 'ID участника');
  field(f, 'role', 'Роль', 'text', 'human', [['human', 'Человек'], ['agent', 'Агент'], ['runner', 'CI']]);
  f = form(d, 'Добавить домен', v => send({
    op: 'domain',
    id: v.get('id'),
    title: v.get('title'),
    authority: v.get('authority')
  }));
  field(f, 'id', 'ID домена');
  field(f, 'title', 'Название');
  field(f, 'authority', 'Кто решает', 'text', '', humans());
  f = form(d, 'Добавить способность', v => send({
    op: 'capability',
    id: v.get('id'),
    title: v.get('title'),
    domain: v.get('domain')
  }));
  field(f, 'id', 'ID способности');
  field(f, 'title', 'Мы умеем…');
  field(f, 'domain', 'Домен', 'text', '', options(data.state.domains));
  d.append(button('Подтвердить дерево', () => send({
    op: 'confirm_tree'
  })));
  f = form(d, 'Добавить репозиторий', v => send({
    op: 'project',
    id: v.get('id'),
    repository: v.get('repository')
  }));
  field(f, 'id', 'ID проекта');
  field(f, 'repository', 'Адрес репозитория');
}
function intake(t) {
  const d = details(t, 'Обращения · ' + Object.keys(data.state.intake).length);
  const f = form(d, 'Сохранить дословно', v => send({
    op: 'intake',
    id: 'i-' + crypto.randomUUID(),
    said: v.get('said'),
    author: v.get('author')
  }));
  field(f, 'author', 'Кто сказал', 'text', actor);
  field(f, 'said', 'Обращение', 'textarea');
  for (const [id, i] of Object.entries(data.state.intake)) {
    const item = details(d, id + ' · ' + i.author);
    item.append(E('p', i.said));
    if (!Object.keys(i.points).length) {
      const split = form(item, 'Разобрать на пункты', v => send({
        op: 'split',
        id,
        points: v.get('points').split('\n').filter(Boolean)
      }));
      field(split, 'points', 'Один пункт на строку', 'textarea', i.said);
    }
    for (const [pid, p] of Object.entries(i.points)) {
      const point = details(item, pid + ' · ' + (p.outcome || 'не разобрано'));
      point.append(E('p', p.said));
      if (p.outcome) {
        point.append(E('p', p.reason));
        continue;
      }
      let f = form(point, 'Назначить род', v => send({
        op: 'sort',
        id,
        point: pid,
        kind: v.get('kind')
      }));
      field(f, 'kind', 'Род', 'text', p.kind, Object.keys(data.state.questions).map(k => [k, k]));
      for (const key of data.state.questions[p.kind] || []) {
        f = form(point, 'Сохранить ответ', v => send({
          op: 'answer',
          id,
          point: pid,
          field: key,
          value: v.get('value')
        }));
        field(f, 'value', key, 'textarea', p.answers[key] || '');
      }
      f = form(point, 'Записать решение', v => send({
        op: 'route',
        id,
        point: pid,
        outcome: v.get('outcome'),
        reason: v.get('reason'),
        req: v.get('outcome') === 'work' ? v.get('req') : null
      }));
      field(f, 'outcome', 'Решение', 'text', 'work', [['work', 'В работу'], ['refused', 'Не делаем'], ['elsewhere', 'Решается иначе']]);
      field(f, 'req', 'Требование', 'text', '', Object.keys(data.state.requirements).map(k => [k, k]));
      field(f, 'reason', 'Причина', 'textarea');
    }
  }
}
function card() {
  const root = $('card');
  root.replaceChildren();
  if (!selected) {
    root.append(E('p', 'Добавьте домен, способность и первое требование.'));
    return;
  }
  const q = data.state.requirements[selected],
    r = q.revisions.find(r => r.number === revisionNumber) || q.revisions.at(-1),
    current = r === q.revisions.at(-1),
    s = current ? data.summary[selected] : null;
  root.append(E('h1', r.title), E('p', selected + ' · редакция ' + r.number + ' · владелец ' + r.owner), E('p', r.body));
  const revisions = E('select');
  for (const old of q.revisions) {
    const o = E('option', 'Редакция ' + old.number + ' · ' + old.reason);
    o.value = old.number;
    revisions.append(o);
  }
  revisions.value = r.number;
  revisions.onchange = e => {
    revisionNumber = +e.target.value;
    card();
  };
  root.append(revisions);
  root.append(E('p', s ? s.accepted ? 'Принято' : s.ready_for_acceptance ? 'Готово к приёмке' : r.confirmed ? 'В работе' : 'Формулировка ждёт подтверждения' : 'Историческая редакция. Факты — в проверках и истории.'));
  const base = {
    req: selected,
    revision: r.number
  };
  if (current && !window.SNAPSHOT) {
    root.append(button('Редактировать REQ + AC', () => edit(r), 'write'), button('Подтвердить формулировку', () => send({
      ...base,
      op: 'confirm'
    }), 'write'));
    if (s.ready_for_acceptance && !s.accepted) root.append(button('Принять результат', () => send({
      ...base,
      op: 'accept',
      run: s.run
    }), 'write'));
  }
  for (const a of r.acs) {
    const item = E('article');
    const status = s?.acs[a.id];
    item.append(E('h3', a.id + ' · ' + a.text), E('span', status?.verification || 'см. историю', 'badge ' + (status?.verification || '')), E('p', status?.ready ? 'Готов к работе' : a.assessment?.reason || 'Ждёт разбора'));
    if (a.before_launch) item.append(E('small', 'Обязательно перед запуском'));
    if (a.decider) item.append(E('p', 'Решение: ' + a.decider));
    if (current && !window.SNAPSHOT) {
      const d = details(item, 'Оценить готовность');
      const f = form(d, 'Записать оценку', v => send({
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
      field(f, 'ready', 'Готовность', 'text', 'no', [['no', 'Ждёт решения'], ['yes', 'Готов к работе']]);
      field(f, 'reason', 'Обоснование или конкретный вопрос', 'textarea', a.assessment?.reason || '');
      field(f, 'recipient', 'Кому вопрос', 'text', '', [['', '—'], ...members()]);
      field(f, 'decision', 'Решение в обсуждении', 'text', '', [['', '—'], ...Object.entries(q.comments).filter(([, c]) => c.kind === 'decision' && !c.withdrawn).map(([id, c]) => [id, c.actor + ': ' + c.text.slice(0, 70)])]);
    }
    root.append(item);
  }
  tasks(root, r, base, current);
  runs(root, r, base, current);
  comments(root, q, r, base);
  const h = details(root, 'История правок и действий');
  for (const ev of data.history.filter(e => e.command.req === selected)) {
    historyEvent(h, ev, q);
  }
}
function historyEvent(parent, event, q) {
  const names = {
    revise: 'Сохранена редакция',
    comment: 'Ответ в обсуждении',
    assess: 'Оценка готовности',
    confirm: 'Подтверждена формулировка',
    task: 'Создана задача',
    complete_task: 'Выполнена задача',
    plan: 'Запечатан план проверок',
    result: 'Результат проверки',
    finish: 'Прогон завершён',
    accept: 'Результат принят',
    revoke: 'Приёмка отозвана',
    issue: 'Дефект подтверждён',
    resolve: 'Исправление подтверждено',
    withdraw_decision: 'Решение отозвано'
  };
  const c = event.command,
    d = details(parent, event.recorded_at + ' · ' + event.actor + ' · ' + (names[c.op] || c.op));
  if (c.op === 'revise') {
    const old = q.revisions[c.expected_revision - 1];
    d.append(E('p', 'Причина: ' + c.reason), E('p', 'Формулировка: ' + (old?.title || '—') + ' → ' + c.title));
    if (old?.body !== c.body) d.append(E('p', 'Контекст: ' + (old?.body || '—') + ' → ' + c.body));
    const ids = new Set([...(old?.acs || []).map(a => a.id), ...c.acs.map(a => a.id)]);
    for (const id of ids) {
      const a = old?.acs.find(a => a.id === id),
        b = c.acs.find(a => a.id === id);
      if (JSON.stringify(a) !== JSON.stringify(b)) d.append(E('p', id + ': ' + (a?.text || '—') + ' → ' + (b?.text || '—')));
    }
  } else d.append(E('p', c.text || c.reason || c.title || c.assessment?.reason || c.evidence?.note || 'Редакция ' + c.revision + (c.run ? ' · прогон ' + c.run : '')));
  const raw = details(d, 'Данные записи');
  raw.append(E('pre', JSON.stringify(c, null, 2)));
}
function edit(r) {
  const root = $('card');
  root.replaceChildren(E('h2', r ? 'Новая редакция — все проверки начнутся с n/a' : 'Новое требование'));
  const f = form(root, 'Сохранить редакцию', v => {
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
      expected_revision: r?.number || 0,
      capability: v.get('capability'),
      title: v.get('title'),
      body: v.get('body'),
      owner: v.get('owner'),
      reason: v.get('reason'),
      acs
    });
  });
  field(f, 'req', 'ID требования', 'text', r ? selected : '').readOnly = !!r;
  field(f, 'capability', 'Способность', 'text', r ? data.state.requirements[selected].capability : '', options(data.state.capabilities));
  field(f, 'title', 'Что должно быть правдой', 'text', r?.title);
  field(f, 'body', 'Контекст', 'textarea', r?.body);
  field(f, 'owner', 'Владелец', 'text', r?.owner || actor, humans());
  field(f, 'reason', 'Причина правки', 'textarea');
  function acrow(a = {}) {
    const row = E('fieldset');
    field(row, 'ac_id', 'Постоянный ID условия', 'text', a.id || '');
    field(row, 'ac_text', 'Условие приёмки, включая нужный клиент', 'textarea', a.text || '');
    field(row, 'decider', 'Нужно решение специалиста', 'text', a.decider || '', [['', 'Не требуется'], ...humans()]);
    field(row, 'depends', 'Зависит от AC (ID через запятую)', 'text', (a.depends_on || []).join(', '));
    field(row, 'before_launch', 'Обязательно до запуска', 'checkbox', a.before_launch);
    row.append(button('Убрать из новой редакции', () => row.remove()));
    f.insertBefore(row, f.querySelector('button[type=submit]'));
  }
  for (const a of r?.acs || [{}]) acrow(a);
  f.append(button('Добавить AC', () => acrow()));
  root.append(button('Отмена', card));
}
function tasks(root, r, base, current) {
  const d = details(root, 'Работа · ' + Object.keys(r.tasks).length);
  for (const [id, t] of Object.entries(r.tasks)) {
    d.append(E('p', id + ' · ' + t.title + ' · ' + t.acs.join(', ') + ' · ' + t.projects.join(', ') + ' · ' + (t.done ? 'Завершено: ' + t.done : 'Исполнитель: ' + t.assignee)));
    if (current && !t.done && !window.SNAPSHOT) {
      const f = form(d, 'Завершить задачу', v => send({
        ...base,
        op: 'complete_task',
        id,
        evidence: v.get('evidence')
      }));
      field(f, 'evidence', t.purpose === 'decision' ? 'ID решения из обсуждения' : 'Что сделано и где результат', 'textarea');
    }
  }
  if (!current || window.SNAPSHOT) return;
  const f = form(d, 'Создать задачу', v => send({
    ...base,
    op: 'task',
    id: v.get('id'),
    title: v.get('title'),
    acs: v.getAll('acs'),
    projects: v.getAll('projects'),
    purpose: v.get('purpose'),
    assignee: v.get('assignee')
  }));
  field(f, 'id', 'ID задачи');
  field(f, 'title', 'Что сделать');
  field(f, 'assignee', 'Исполнитель', 'text', actor, members());
  field(f, 'purpose', 'Вид работы', 'text', 'implementation', [['implementation', 'Реализация'], ['manual', 'Настройка / ручная работа'], ['decision', 'Получить решение']]);
  const a = field(f, 'acs', 'Условия', 'text', '', r.acs.map(a => [a.id, a.id]));
  a.multiple = true;
  const p = field(f, 'projects', 'Репозитории (можно не выбирать)', 'text', '', options(data.state.projects));
  p.multiple = true;
}
function runs(root, r, base, current) {
  const d = details(root, 'Проверки · ' + Object.keys(r.runs).length);
  for (const [id, run] of Object.entries(r.runs)) {
    const block = details(d, id + ' · ' + (run.finished ? 'Завершён' : 'Идёт'));
    block.append(E('p', 'Предмет: ' + JSON.stringify(run.subject)), E('p', 'Область: ' + run.scope + ' · сбор ' + (run.discovery_complete ? 'полный' : 'неполный')));
    for (const c of run.plan) {
      const result = run.results[c.id];
      const recorded = data.history.find(e => e.command.op === 'result' && e.command.req === base.req && e.command.revision === base.revision && e.command.run === id && e.command.evidence.check === c.id);
      if (recorded) block.append(E('small', recorded.actor + ' · ' + recorded.recorded_at));
      block.append(E('h3', c.id), E('p', 'Ожидали: ' + c.expected.join(', ')), E('p', result ? 'Получили: ' + result.observed.join(', ') + ' · ' + result.outcome + ' · ' + result.note : 'Результата нет'));
      if (!run.finished && !result && c.method === 'manual' && !window.SNAPSHOT) {
        const f = form(block, 'Записать проверку', v => send({
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
        field(f, 'outcome', 'Результат', 'text', 'pass', [['pass', 'Пройдено'], ['fail', 'Не пройдено'], ['skip', 'Пропущено']]);
        field(f, 'observed', 'Фактический список — одна позиция на строку', 'textarea');
        field(f, 'note', 'Что и как проверено', 'textarea');
        field(f, 'link', 'Ссылка на свидетельство');
      }
    }
    if (!run.finished && !window.SNAPSHOT) block.append(button('Завершить прогон', () => send({
      ...base,
      op: 'finish',
      run: id
    })));
  }
  if (current && !window.SNAPSHOT) {
    const f = form(d, 'Запечатать план ручной проверки', v => send({
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
    field(f, 'id', 'ID прогона');
    field(f, 'subject', 'Что проверяем: среда и версия настроек');
    field(f, 'scope', 'Область проверки');
    field(f, 'complete', 'Все проверки в этой области собраны', 'checkbox');
    for (const a of r.acs) field(f, 'expected-' + a.id, a.id + ' · ожидаемый список', 'textarea');
  }
}
function comments(root, q, r, base) {
  const d = details(root, 'Обсуждение');
  d.open = true;
  for (const [id, c] of Object.entries(q.comments).filter(([, c]) => c.revision === r.number)) {
    d.append(E('small', id + ' · ' + c.actor + ' · ' + c.kind + (c.withdrawn ? ' · отозвано' : '') + (c.reply_to ? ' · ответ на ' + c.reply_to : '')), E('p', c.text));
  }
  if (window.SNAPSHOT) return;
  const f = form(d, 'Ответить в карточке', v => send({
    ...base,
    op: 'comment',
    id: 'c-' + crypto.randomUUID(),
    text: v.get('text'),
    kind: v.get('kind'),
    reply_to: v.get('reply_to') || null,
    ac: v.get('ac') || null
  }));
  field(f, 'kind', 'Тип ответа', 'text', 'comment', [['comment', 'Комментарий / замечание'], ['decision', 'Решение специалиста'], ['accept', 'Согласен с предложением'], ['change', 'Уточняю предложение'], ['reject', 'Отклоняю с причиной'], ['correction', 'Исправляю прежний ответ']]);
  field(f, 'text', 'Ответ', 'textarea');
  field(f, 'reply_to', 'На какой комментарий (ID)');
  field(f, 'ac', 'Условие', 'text', '', [['', 'Всё требование'], ...r.acs.map(a => [a.id, a.id])]);
}
if (window.SNAPSHOT) {
  document.body.classList.add('snapshot');
  $('login').remove();
  $('refresh').remove();
  data = window.SNAPSHOT;
  ws = data.workspace;
  render();
}
