-- joppa — дерево обязательств продукта.
--
-- Файл ПОЛНЫЙ: поднимает систему с нуля одной командой. Миграций нет намеренно —
-- пока это прототип, база пересоздаётся целиком:
--
--   docker exec -i joppa-db psql -U postgres -c 'drop database if exists joppa'
--   docker exec -i joppa-db psql -U postgres -c 'create database joppa'
--   docker exec -i joppa-db psql -U postgres -d joppa -f - < schema.sql
--
-- Правило, которое нарушать нельзя: ни одной таблицы и ни одной колонки мимо
-- этого файла. Иначе развёрнутое с нуля и работающее расходятся, и ревью
-- проверяет не ту систему, что запущена. Так уже было.

drop table if exists checks, answers, items, criteria, needs, intake, nodes cascade;
drop sequence if exists intake_no;

-- ── ДЕРЕВО ────────────────────────────────────────────────────────────────
-- Domain → Capability → Requirement. Одна таблица, а не три: уровни отличаются
-- только положением, и три таблицы заставили бы писать три одинаковых запроса
-- ради одного обхода.
create table nodes (
  id         text primary key,
  kind       text not null check (kind in ('domain','capability','requirement')),
  parent_id  text references nodes(id),
  title      text not null,
  body       text,
  source     text,
  taken_by   text,
  taken_at   timestamptz,
  done_at    timestamptz,
  created_at timestamptz not null default now(),
  -- Домен растёт из ничего, остальные обязаны иметь родителя: требование,
  -- висящее в воздухе, не отвечает на вопрос «какой частью бизнеса это является».
  constraint shape check (
    (kind = 'domain'  and parent_id is null) or
    (kind <> 'domain' and parent_id is not null)),
  constraint no_self_parent check (parent_id is distinct from id),
  -- Границы длины. Заголовок требования — 140: одно утверждение, одно
  -- предложение. Длиннее значит, что внутри спрятался союз «и», то есть два
  -- требования. Домен и капабилити — 60, должны влезать в заголовок.
  constraint title_fits check (
    char_length(title) <= case kind when 'requirement' then 140 else 60 end),
  -- Тело — контекст, а не документ: подробности живут в источнике.
  constraint body_is_context check (body is null or char_length(body) <= 800)
);
create index nodes_of_parent on nodes(parent_id);

-- ── AC ────────────────────────────────────────────────────────────────────
-- Acceptance criteria: конкретные проверяемые утверждения внутри требования.
-- В работу берут ИХ, а не требование целиком: «порог 10 000» и «курс хранится
-- рядом с транзакцией» делают разные люди в разное время, но живут под одним
-- обязательством и проверяются вместе.
create table criteria (
  id       bigserial primary key,
  node_id  text not null references nodes(id) on delete cascade,
  ord      int  not null,
  text     text not null,
  taken_by text,
  done_at  timestamptz,
  unique (node_id, ord),
  -- 110 символов: AC должен проверяться одним действием и читаться одним
  -- взглядом. Не влезает — это НЕ один AC, а два, и дробить надо при
  -- написании, а не при проверке.
  constraint ac_is_one_check check (char_length(text) <= 110)
);
create index criteria_of_node on criteria(node_id, ord);

-- ── ПРИЁМ ─────────────────────────────────────────────────────────────────
-- Обращение человека. Не требует ничего и не отвергается никогда: сырое —
-- нормальное состояние. Хранится дословно, чтобы потом сверять, не подменили
-- ли смысл при разборе.
create sequence intake_no;

create table intake (
  id      text primary key,
  said    text not null,
  author  text not null,
  said_at timestamptz not null default now()
);

-- Пункт обращения. ЕДИНИЦА РАЗБОРА, а не обращение целиком: живая просьба
-- почти никогда не однородна. «Сделайте четыре события» на деле означает
-- «два делаем, одно не делаем совсем, одно решается иначе». Если род и судьбу
-- назначать обращению, эти четыре судьбы схлопнутся в одну, а вместе с ними
-- пропадёт причина отказа.
create table items (
  id           text primary key,
  intake_id    text not null references intake(id) on delete cascade,
  ord          int  not null,
  said         text not null,
  kind         text,
  kind_at      timestamptz,
  outcome      text,
  outcome_why  text,
  outcome_by   text,
  outcome_at   timestamptz,
  node_id      text   references nodes(id),
  criterion_id bigint references criteria(id) on delete set null,
  unique (intake_id, ord),
  constraint outcome_known check (outcome is null or outcome in ('work','refused','elsewhere')),
  constraint outcome_complete check (num_nulls(outcome, outcome_by, outcome_at) in (0,3)),
  -- Отказ и «решается иначе» обязаны назвать причину: без неё через месяц никто
  -- не вспомнит, почему не сделали, и пункт заведут заново.
  constraint refusal_names_why check (
    outcome not in ('refused','elsewhere') or outcome_why is not null)
);
create index items_of_intake on items(intake_id, ord);
create index items_of_criterion on items(criterion_id) where criterion_id is not null;

-- Какие вопросы задавать какому роду обращения. Лежит в базе, а не в промпте:
-- меняется политика приёма — правится строка, а не инструкция агенту.
create table needs (
  kind     text not null,
  field    text not null,
  ask      text not null,
  why      text,
  required boolean not null default true,
  ord      int not null default 100,
  primary key (kind, field)
);

create table answers (
  item_id     text not null references items(id) on delete cascade,
  field       text not null,
  value       text not null,
  answered_by text not null,
  answered_at timestamptz not null default now(),
  primary key (item_id, field)
);

insert into needs(kind, field, ask, why, required, ord) values
 ('bug','observed','что именно вы увидели','без этого нечего воспроизводить',true,1),
 ('bug','expected','что должно было быть вместо этого','это и есть нарушенное требование',true,2),
 ('bug','where','где это было: экран, стенд, кто из клиентов','чтобы найти, к какому месту относится',true,3),
 ('bug','when','когда заметили',null,false,4),

 ('policy','rule','какое правило должно действовать','это станет формулировкой требования',true,1),
 ('policy','authority','кто уполномочен это решить','решение принимает не разработка',true,2),
 ('policy','applies_to','на кого или на что распространяется','иначе непонятны границы',true,3),
 ('policy','lifts_when','при каких условиях пересматриваем','чтобы правило не осталось навсегда молча',false,4),

 ('question','unknown','что именно неизвестно','вопрос без предмета нельзя закрыть',true,1),
 ('question','decides','какое решение зависит от ответа','иначе это любопытство, а не работа',true,2),

 ('idea','outcome','что станет правдой, если сделаем','это станет требованием',true,1),
 ('idea','today','как это работает сегодня','чтобы понять, что именно меняем',true,2),
 ('idea','who','для кого это улучшение',null,false,3);
