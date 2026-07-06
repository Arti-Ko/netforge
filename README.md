<div align="center">

<img src=".github/assets/icon.png" width="120" alt="NetForge" />

# NetForge

**Фабрика персональных VPN-конфигов для macOS.**
Подключается к твоему серверу по SSH, показывает все ссылки Hysteria2
с описанием и сроком жизни — и создаёт новые в пару кликов.

<sub>Просроченные конфиги удаляются с сервера сами. Никакой ручной правки `config.yaml`.</sub>

![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-backend-000000?logo=rust&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-frontend-3178C6?logo=typescript&logoColor=white)
![Platform](https://img.shields.io/badge/Platform-macOS-000000?logo=apple&logoColor=white)

</div>

---

## Скриншоты

<!--
  Положи реальные скриншоты в .github/assets/ и раскомментируй блок ниже.
  Снимаются на ⌘⇧4 + пробел (окно целиком с тенью).
-->
<div align="center">
<!--
<img src=".github/assets/screenshot-list.png"   width="49%" alt="Список конфигов" />
<img src=".github/assets/screenshot-create.png" width="49%" alt="Создание конфига" />
-->
<i>Скриншоты появятся здесь.</i>
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
- 🔀 **coffee://bundle (hysteria2 + VLESS)** — если на том же сервере стоит x-ui
  с VLESS Reality инбаундом, при **создании** конфига NetForge заводит юзеру и
  hysteria2-аккаунт, и персональный VLESS-клиент (новый UUID) в x-ui, а затем
  выдаёт бандл `coffee://bundle?w=<hysteria2>&m=<vless>`. Клиент coffeeNetwork
  сам выбирает плечо по сети: **WiFi → hysteria2** (`w`), **мобильный → VLESS**
  (`m`, устойчив к DPI). Существующие юзеры подбираются к VLESS по имени
  (email в инбаунде); pbk выводится из privateKey через `xray x25519`. Правки
  x-ui идут через БД с автобэкапом; для хостов без x-ui берётся статичный
  VLESS-link из настроек (fallback), либо выдаётся чистый hysteria2.

## Как это работает

```
┌───────────────┐   SSH (ключ)   ┌─────────────────────────────────┐
│   NetForge    │ ─────────────▶ │  VPS: /etc/hysteria/config.yaml  │
│ (Tauri/macOS) │   read / edit  │  auth.userpass  → restart svc    │
└───────────────┘                └─────────────────────────────────┘
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

End-to-end проверка SSH-бэкенда против реального сервера. Параметры подключения
берутся из переменных окружения — **ничего не зашито в код**:

```bash
cd src-tauri
NF_HOST=203.0.113.10 NF_SNI=vpn.example.com \
  cargo run --example verify
```

Прогоняет `connect → read → add → remove` по ключу `~/.ssh/id_ed25519`
(или `NF_KEY`).

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
auth-тип `userpass`) и доступом по SSH-ключу.

## Семейство coffeeNetwork

NetForge — серверная половина связки: он **создаёт** конфиги, а клиенты их
**используют**.

- [**coffeeNetwork**](https://github.com/Arti-Ko/coffeeNetwork) — десктопный
  VPN-клиент (macOS / Windows / Linux).
- [**coffeeNetwork-android**](https://github.com/Arti-Ko/coffeeNetwork-android) —
  Android-клиент на sing-box.

---

<sub>Адреса серверов и пароли хранятся только локально на твоей машине —
в репозитории их нет. Вставь свои данные в настройках при первом запуске.</sub>
