# joppa

**Дерево обязательств продукта: domain → capability → requirement → AC.**
Один воркспейс — один продукт, даже если его делают в четырёх репозиториях.
Репозитории указываются у задач; бизнес-дерево описывает поведение продукта.

REQ и AC формулируют вместе люди и агент. Владелец REQ подтверждает формулировку
и принимает проверенный результат. Агент оценивает готовность каждого AC:
независимые условия идут в работу, спорные ждут конкретного человека.
iOS в условии означает iOS; другие клиенты сами не добавляются.

## Что работает в 0.2

- Воркспейсы, участники, домены с полномочием, способности, REQ с владельцем и AC.
  Агент предлагает дерево, владелец продукта подтверждает.
- Приём дословного обращения, разбор на пункты, вопросы по роду и три исхода:
  в работу, не делаем с причиной, решается иначе. Баг связывается с существующим REQ.
- Карточка REQ: редакции, обсуждение, решения специалиста, ответы владельца,
  история авторов и времени. Новая редакция начинает с **n/a по всем AC**.
- Задачи по AC в нескольких репозиториях, ручная работа и получение решения.
  Задача выполнена ≠ AC проверен. Ручная проверка хранит ожидаемый и фактический списки.
- Запечатанные планы проверок, результаты, приёмка владельцем, отзыв решения,
  подтверждённые дефекты и условия запуска. Журнал дополняется, история не переписывается.

## Запуск

Нужны Rust 1.88+, PostgreSQL 16 и Python 3 для примеров/адаптера проверок.
Для нового локального PostgreSQL (если контейнера ещё нет):

```sh
docker run -d --name joppa-db -e POSTGRES_PASSWORD=joppa \
  -p 127.0.0.1:55433:5432 -v joppa-data:/var/lib/postgresql/data postgres:16
# Когда pg_isready сообщил accepting connections:
docker exec joppa-db pg_isready -U postgres
docker exec joppa-db createdb -U postgres joppa
```

Для существующей базы начинайте здесь. Миграция добавляет схему `joppa`,
не удаляет и не переносит старые таблицы из `public`.

```sh
cargo build --locked
export JOPPA_DATABASE_URL=postgresql://postgres:joppa@127.0.0.1:55433/joppa
./target/debug/joppa migrate
export JOPPA_ACTOR=owner
printf '%s\n' '{"op":"workspace","title":"Мой продукт"}' | \
  ./target/debug/joppa command product create-product
```

Для API и автоматизации создайте личный файл соответствия токенов участникам:

```sh
python3 - <<'PY'
import json, os, secrets
path = '/tmp/joppa-tokens.json'
with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as f:
    json.dump({secrets.token_urlsafe(32): 'owner'}, f)
print('API credentials saved to', path)
PY
export JOPPA_TOKENS_FILE=/tmp/joppa-tokens.json
./target/debug/joppa serve 6212
```

Browser sign-in talks directly to Google using Joppa's own OAuth client
and invitation list.

Before starting `serve`, set `JOPPA_GOOGLE_CLIENT_FILE` to a private JSON file:

```json
{
  "client_id": "YOUR_JOPPA_CLIENT.apps.googleusercontent.com",
  "client_secret": "YOUR_JOPPA_CLIENT_SECRET",
  "redirect_uri": "https://YOUR_JOPPA_DOMAIN/auth/google/callback"
}
```

Register that exact callback in Joppa's Google Cloud OAuth client. HTTPS is
required outside localhost. Set `JOPPA_LOGIN_USERS_FILE` to a private JSON file
mapping invited Google email addresses to existing Joppa actors, for example
`{"owner@example.com": "owner"}`. Each actor must exist in the API identity file.
Files containing secrets must be readable only by the service account.

Open Joppa and choose **Continue with Google**. Complete sign-in in the Google
window; the board opens automatically. Until sign-in, only the authorization
screen is visible. The server verifies the token signature, audience, issuer,
expiry, nonce and email verification before applying Joppa's own invitation list.
OAuth uses a browser-bound, single-use state and PKCE. Browser sessions have
separate HttpOnly cookies, expire after 12 hours, and are invalidated on sign-out
or server restart. Google credentials and API tokens never reach browser storage.
Without Joppa's own Google configuration, sign-in fails closed with a setup message.

The **Workspace** menu switches products. [How it works](http://127.0.0.1:6212/guide)
explains domains, capabilities, requirements and acceptance criteria.
Названия доменов и capabilities открывают их карточки. В карточке можно
собрать дерево, редактировать REQ и AC, обсуждать, создать задачи, провести
ручную проверку и принять результат. Если старый сервер уже занимает 6212,
запустите новый на `6213`. Публичный доступ требует своего HTTPS-прокси.

## CLI, MCP и HTTP

```sh
./target/debug/joppa list
./target/debug/joppa read product
./target/debug/joppa command product unique-request-id < command.json
./target/debug/joppa board product /tmp/product.html
./target/debug/joppa gate product < target-subject.json
```

`command` принимает JSON с `op`; все формы — в [docs/api.md](docs/api.md).
Повтор с тем же request ID и содержимым безопасен; другое содержимое отвергается.
`gate` завершается с ошибкой, пока обязательные до запуска AC не приняты
или целевые сборки/настройки отличаются от проверенных. Это проверка для
вашего deploy-скрипта; сама joppa ничего не выкатывает.

Без аргументов бинарь обслуживает MCP по stdio:

```json
{
  "joppa": {
    "type": "stdio",
    "command": "/path/to/joppa/target/debug/joppa",
    "env": {
      "JOPPA_DATABASE_URL": "postgresql://postgres:joppa@127.0.0.1:55433/joppa",
      "JOPPA_ACTOR": "agent"
    }
  }
}
```

Сначала владелец добавляет участника `agent` с ролью `agent`.
Инструменты: `joppa_list`, `joppa_read`, `joppa_help`, `joppa_command`.
HTTP использует Bearer-токен; автор команды не передаётся её аргументами.
CLI/MCP доверяют запускающему процессу, который назначает `JOPPA_ACTOR`.
Участник с доступом к серверной конфигурации или администратор БД находится
за этой границей доверия; токен человека агенту не выдают.

## Автоматические проверки

Номер AC находится в имени теста: `[ios-access] paid access survives login`.
Один тест — один AC, тестов на AC может быть несколько. До исполнения
отправляется весь ожидаемый набор и подтверждение полноты сбора.

```sh
JOPPA_ACTOR=ci python3 scripts/run_checks.py product examples/checks.json \
  --joppa ./target/debug/joppa
```

Замените демонстрационные версии и команду настоящими. Адаптер запечатывает
план, выполняет команды без shell, сохраняет результат каждого теста и завершает
прогон. Смешанный план с ручными проверками передаётся через API.
Удалённая/пропущенная проверка и неполный сбор не дают зелени. Достаточность
и смысл тестов остаются предметом приёмки человеком.

## Хранение и переход с 0.1

`migrations/001_journal.sql` — единственный источник новой схемы; `schema.sql`
вызывает ту же миграцию для psql. Повторный запуск безопасен.
`joppa.events` хранит неизменяемые команды с автором и серверным временем.
Состояние восстанавливается из журнала. Запись сериализуется в пределах
воркспейса; редакции дополнительно используют `expected_revision`.

Триггеры запрещают UPDATE, DELETE и TRUNCATE журнала. Для серверного процесса
используйте роль с SELECT/INSERT и доступом к последовательности, без владения
схемой и таблицами; пример — [docs/storage.md](docs/storage.md).
Это WORM на уровне приложения и обычных SQL-операций. Администратор PostgreSQL
способен отключить триггеры; физическое неизменяемое архивирование — отдельная инфраструктура.

Старые данные доступны через `joppa legacy serve 6214` и прежние MCP-инструменты
через `joppa legacy`. Старый режим сохраняет ограничения 0.1 и **не является
путём записи новой модели**. Автоматически объявлять старые done_at проверками,
назначать владельцев и подтверждать реальные правила миграция не пытается.

## Проверить реализацию

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
docker exec joppa-db createdb -U postgres joppa_test_local
JOPPA_TEST_DATABASE_URL=postgresql://postgres:joppa@127.0.0.1:55433/joppa_test_local \
  cargo test --test postgres -- --ignored
```

Интеграционный тест требует отдельной базы с префиксом `joppa_test` и проверяет
гонки правок/результатов/приёмки, изоляцию, идемпотентность, повтор миграции,
сохранность прежних таблиц и запрет изменения истории. CI поднимает PostgreSQL 16.

## Границы версии

Готов вертикальный цикл: сформулировать → решить → выполнить → проверить → принять.
Подключение CI конкретных репозиториев и перенос существующих данных требуют
содержательного разбора владельцем. Готового SSO, фонового исполнителя задач,
доказательства полноты AC и автоматического определения юридических правил нет.
Журнал сейчас читается целиком на воркспейс; для больших объёмов понадобятся
снимки проекций и постраничная история. Интерфейс покрывает основной цикл;
отзыв решения, подтверждение дефекта и административные операции доступны также через API/MCP.

Модель — [model](docs/model.md), процесс — [process](docs/process.md),
правило зелени — [green](docs/green.md), разбор прежнего прототипа — [reviews](docs/reviews.md).
