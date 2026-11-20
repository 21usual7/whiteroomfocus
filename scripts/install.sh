#!/usr/bin/env bash
# Установка WHITE ROOM из распакованного архива в пользовательский каталог (по умолчанию ~/.local).
#   ./install.sh                 установить или обновить
#   PREFIX=/tmp/x ./install.sh   установить в другой каталог
# База не удаляется и не перезаписывается. При первом запуске новой версии CLI или окно сами делают
# копию <база>.pre-v<схема>.bak перед миграцией. Расширение и native messaging не трогаются:
# для них — whiteroom browser-setup и «Загрузить распакованное» (README, «Браузер»).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
share=$prefix/share/whiteroom
new=$("$here/bin/whiteroom" --version)
if command -v "$prefix/bin/whiteroom" >/dev/null 2>&1; then
  old=$("$prefix/bin/whiteroom" --version 2>/dev/null || echo "0.1.0 (MVP)")
  echo "Обновление $old → $new"
  db=${WHITEROOM_DB:-${XDG_DATA_HOME:-$HOME/.local/share}/whiteroom/whiteroom.db}
  if [ -f "$db" ]; then
    "$prefix/bin/whiteroom" backup "$db.before-$new.bak" 2>/dev/null \
      || cp "$db" "$db.before-$new.bak"
    echo "Копия базы: $db.before-$new.bak"
  fi
fi
apps=${XDG_DATA_HOME:-$HOME/.local/share}/applications
[ -n "${PREFIX:-}" ] && apps=$prefix/share/applications
mkdir -p "$prefix/bin" "$share" "$apps"
install -m 755 "$here/bin/whiteroom" "$here/bin/whiteroom-app" "$prefix/bin/"
rm -rf "$share/extension" "$share/modules"
cp -r "$here/extension" "$here/modules" "$share/"
cat > "$apps/whiteroom.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=WHITE ROOM
Exec=$prefix/bin/whiteroom-app
Categories=Education;
DESK
echo "Установлено $new в $prefix. Модули: $share/modules, расширение: $share/extension"
echo "Проверка: $prefix/bin/whiteroom --version && $prefix/bin/whiteroom today"
