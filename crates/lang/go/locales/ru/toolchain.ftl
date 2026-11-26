# Zen toolchain: ошибки команд инструментария. Перевод с английского.
toolchain-error-none = В этой IDE нет инструментария языков.
toolchain-error-unknown-sdk = { $root } — не Go SDK, найденный на этом компьютере или добавленный.
toolchain-error-no-go = { $root } — не Go SDK: в нём нет bin/go.
toolchain-error-no-version = { $root } — не Go SDK: его команда go не сообщает версию.
toolchain-error-not-added = { $root } — не Go SDK, добавленный вручную.
toolchain-error-no-gopls = { $path } не существует.
toolchain-error-not-gopls = { $path } — не gopls: он не отвечает на «gopls version».
toolchain-error-no-sdk = Go SDK не установлен.
toolchain-error-no-sdk-install = Go SDK не установлен; сначала установите Go с https://go.dev/dl/.
toolchain-error-gopls-missing = gopls не установлен.
toolchain-error-installed-not-found = go install завершился, но gopls не найден в GOPATH/bin.
toolchain-error-save = Не удалось сохранить { $file }: { $error }
toolchain-error-run = Не удалось запустить { $program }: { $error }
toolchain-error-no-dlv = { $path } не существует.
toolchain-error-not-dlv = { $path } — не Delve: он не отвечает на «dlv version».
toolchain-error-dlv-installed-not-found = go install завершился, но dlv не найден в GOPATH/bin.
toolchain-error-agent = Агент не может добавлять, удалять или выбирать Go SDK, gopls и Delve: это выбор запускаемой программы. Человек может сделать это в разделе «Настройки → Языки и фреймворки → Go».
