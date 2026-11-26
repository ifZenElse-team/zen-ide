# Модуль Rust: его название и описание.
module-rust-name = Rust
module-rust-description = Всё для Rust: подсветка, рабочие пространства и крейты Cargo, rust-analyzer (автодополнение, навигация, ошибки, рефакторинги), инструменты Rust и их настройки.

# Модель проекта Cargo.
rust-libraries-crates = Крейты
rust-model-no-cargo = Rust не установлен: cargo не найден. Установите Rust с https://rustup.rs/.
rust-model-offline = { $message } (IDE читает проекты Cargo без сети: скачайте зависимости командой `cargo fetch` или сборкой, затем выполните «Перезагрузить модель проекта»)
rust-toolchain-error-none = В этой IDE нет инструментов Rust.

file-type-rust = Rust

rust-postfix-let = Сохраняет выражение в новую переменную
rust-postfix-if = Выполняет блок, если выражение истинно
rust-postfix-else = Выполняет блок, если выражение ложно
rust-postfix-not = Отрицает выражение
rust-postfix-while = Цикл, пока выражение истинно
rust-postfix-for = Цикл по элементам выражения
rust-postfix-match = match по выражению
rust-postfix-ifl = Выполняет блок, если выражение — Some
rust-postfix-ref = Заимствует выражение
rust-postfix-refm = Заимствует выражение для изменения
rust-postfix-some = Оборачивает выражение в Some
rust-postfix-ok = Оборачивает выражение в Ok
rust-postfix-err = Оборачивает выражение в Err
rust-postfix-box = Помещает выражение в Box
rust-postfix-dbg = Печатает выражение через dbg!
rust-postfix-println = Печатает выражение
rust-postfix-return = Возвращает выражение
rust-postfix-par = Заключает выражение в скобки
rust-tests-framework-builtin = Встроенные #[test]
