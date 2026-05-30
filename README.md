<div align="center">

# ⬡ NetForge

**Фабрика персональных VPN-конфигов для macOS.**
Подключается к твоему серверу по SSH, показывает все ссылки Hysteria2
с описанием и сроком жизни — и создаёт новые в пару кликов.

`Tauri 2` · `Rust` · `TypeScript` · `Vite`

</div>

---

## Что умеет

- 📋 **Список конфигов** — все ссылки `hysteria2://…` с именем, описанием и
  бейджем срока: `БЕССРОЧНО` · `ЕЩЁ N ДН.` · `ИСТЁК`.
- ➕ **Создать** — имя + описание + срок жизни (1 / 7 / 30 / 90 дней или
  бессрочно). На сервере заводится пользователь со сгенерированным паролем,
  готовая ссылка сразу копируется в буфер.
- ⏳ **Срок жизни** — Hysteria2 нативно не умеет expiry, поэтому он хранится
  локально, а **просроченные конфиги автоматически удаляются с сервера при
  запуске** приложения.
- 📎 **Копировать / удалить** прямо из списка (удаление — двойным кликом).

## Как это работает

```
┌───────────────┐   SSH (ключ)   ┌──────────────────────────────┐
│   NetForge    │ ─────────────▶ │  VPS: /etc/hysteria/config.yaml │
│ (Tauri/macOS) │   read / edit  │  auth.userpass  → restart svc   │
└───────────────┘                └──────────────────────────────┘
        │ локально: settings.json + meta.json (описания, сроки)
```

- SSH — через системный `ssh` по ключу (как splitbox запускает `sing-box`).
- Бэкенд (`src-tauri/src/ssh.rs`) читает и **построчно** правит блок
  `auth.userpass`, делает бэкап и `systemctl restart hysteria-server`.
- Метаданные — в `~/Library/Application Support/com.purrweb.netforge/`.

## Стек

| Слой | Технология |
|------|-----------|
| Оболочка | Tauri 2 (нативное окно macOS + webview) |
| Бэкенд | Rust — SSH, парсер/редактор YAML, хранилище |
| Фронтенд | Vanilla TypeScript + Vite |
| Стили | Vanilla CSS (oklch, glass, SF Mono) — дизайн из coffeeNetwork |

## Запуск

```bash
npm install
npm run tauri dev      # дев-режим
npm run tauri build    # сборка .app / .dmg
```

При первом запуске откроются настройки сервера. Дальше приложение само
подтянет список конфигов.

## Проверка бэкенда

```bash
cd src-tauri && cargo run --example verify
```
Прогоняет `connect → read → add → remove` против сервера по ключу
`~/.ssh/id_ed25519`.

## Структура

```
index.html                     разметка (список + модалки create/settings)
src/main.ts                    UI-логика, invoke-вызовы
src/styles.css                 дизайн-система (oklch, glass, mono)
src-tauri/src/lib.rs           Tauri-команды (load / create / delete + expiry)
src-tauri/src/ssh.rs           SSH + парсер/редактор Hysteria2-конфига
src-tauri/src/store.rs         settings.json + meta.json
src-tauri/examples/verify.rs   e2e-проверка бэкенда
```

## Требования к серверу

VPS с установленным **Hysteria2** (standalone, `/etc/hysteria/config.yaml`,
auth-тип `userpass`) и доступом по SSH-ключу. Подробнее о настройке такого
сервера — см. твою VPN-инфраструктуру.

---

<sub>Личный инструмент. Хранит доступы к серверу — держи репозиторий приватным.</sub>
