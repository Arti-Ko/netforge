# NetForge (desktop)

macOS-приложение для выдачи персональных VPN-ссылок. Подключается к серверу по
**SSH**, показывает список всех конфигов **Hysteria2** с описанием и сроком жизни,
и в пару кликов создаёт новые. Стек и дизайн — как у CoffeeNetwork-десктопа
(splitbox): **Tauri 2 + ванильный TypeScript + Vite + ванильный CSS** (oklch,
glass-карточки, SF Mono, янтарный акцент).

## Механика

- **Список**: все ссылки `hysteria2://…` + имя, описание, бейдж срока (бессрочно /
  ещё N дней / истёк). Кнопки: копировать, удалить (двойной клик).
- **Создать**: имя + описание + срок жизни (1/7/30/90 дней или бессрочно) →
  на сервере добавляется пользователь со сгенерированным паролем, метаданные
  сохраняются локально, ссылка сразу копируется в буфер.
- **Срок жизни**: Hysteria2 не умеет expiry нативно, поэтому он хранится локально
  (`~/Library/Application Support/com.purrweb.netforge/meta.json`), а при каждом
  запуске приложения **просроченные пользователи автоматически удаляются с сервера**.

## Как работает

- SSH — через системный `ssh` (по ключу), как splitbox запускает `sing-box`.
  Бэкенд (`src-tauri/src/ssh.rs`) читает и построчно правит
  `/etc/hysteria/config.yaml` (блок `auth.userpass`), делает бэкап и
  `systemctl restart hysteria-server`.
- Настройки сервера (host / SSH-юзер / порт / SNI / путь к ключу) — в
  `settings.json` в том же каталоге.

## Запуск

```bash
npm install
npm run tauri dev          # запуск в дев-режиме
npm run tauri build        # сборка .app/.dmg под macOS
```

При первом запуске откроются настройки — укажи сервер (host, SNI, SSH-доступ).
Дальше приложение само подтянет список конфигов.

## Проверка backend на реальном сервере

```bash
cd src-tauri && cargo run --example verify
```
Прогоняет connect → read → add → remove по ключу `~/.ssh/id_ed25519`.

## Структура

```
index.html              — разметка (список + модалки create/settings)
src/main.ts             — UI-логика, invoke-вызовы
src/styles.css          — дизайн-система (oklch, glass, mono)
src-tauri/src/lib.rs    — Tauri-команды (load/create/delete + прунинг expiry)
src-tauri/src/ssh.rs    — SSH + парсер/редактор Hysteria2-конфига
src-tauri/src/store.rs  — settings.json + meta.json
src-tauri/examples/verify.rs — e2e-проверка бэкенда
```
