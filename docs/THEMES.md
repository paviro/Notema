# Themes

Notema ships with 26 bundled themes. Each is a TOML file named after its
file stem, written to `<config-dir>/themes/` on first use (`~/.config/notema/themes/`
on Linux, `~/Library/Application Support/de.paviro.notema/themes/` on macOS) and
never touched again, so your edits survive upgrades. Switch or preview them live in
the in-app picker; if the configured theme is missing or broken the app warns on
stderr and always falls back to `blossom`.

`classic` is the plain terminal-default look and the safest pick for e-ink displays;
`eclipse` is pure black-and-white and suits e-ink too, wherever the terminal renders
its inversions faithfully (some emulators, Termux among them, don't). The rest are
high-color.

Want to build your own? See the **[theme reference](THEME-REFERENCE.md)** for the
file format and the full token reference. Each name below links to its bundled
definition.

## Gallery

<table>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/blossom.toml"><b>blossom</b></a> — <sub>default</sub><br>
      <img src="../.github/assets/themes/blossom.svg" alt="blossom theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/journal.toml"><b>journal</b></a><br>
      <img src="../.github/assets/themes/journal.svg" alt="journal theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/classic.toml"><b>classic</b></a> — <sub>terminal-default · e-ink safe</sub><br>
      <img src="../.github/assets/themes/classic.svg" alt="classic theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/eclipse.toml"><b>eclipse</b></a> — <sub>monochrome · e-ink</sub><br>
      <img src="../.github/assets/themes/eclipse.svg" alt="eclipse theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/fjord.toml"><b>fjord</b></a><br>
      <img src="../.github/assets/themes/fjord.svg" alt="fjord theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/grove.toml"><b>grove</b></a><br>
      <img src="../.github/assets/themes/grove.svg" alt="grove theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/matcha.toml"><b>matcha</b></a><br>
      <img src="../.github/assets/themes/matcha.svg" alt="matcha theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/indigo.toml"><b>indigo</b></a><br>
      <img src="../.github/assets/themes/indigo.svg" alt="indigo theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/maple.toml"><b>maple</b></a><br>
      <img src="../.github/assets/themes/maple.svg" alt="maple theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/celadon.toml"><b>celadon</b></a><br>
      <img src="../.github/assets/themes/celadon.svg" alt="celadon theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/tokyonight.toml"><b>tokyonight</b></a><br>
      <img src="../.github/assets/themes/tokyonight.svg" alt="tokyonight theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/lavender.toml"><b>lavender</b></a><br>
      <img src="../.github/assets/themes/lavender.svg" alt="lavender theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/rose-pine.toml"><b>rose-pine</b></a><br>
      <img src="../.github/assets/themes/rose-pine.svg" alt="rose-pine theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/dungeon.toml"><b>dungeon</b></a><br>
      <img src="../.github/assets/themes/dungeon.svg" alt="dungeon theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/synthwave.toml"><b>synthwave</b></a><br>
      <img src="../.github/assets/themes/synthwave.svg" alt="synthwave theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/crt.toml"><b>crt</b></a><br>
      <img src="../.github/assets/themes/crt.svg" alt="crt theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/cyberpunk.toml"><b>cyberpunk</b></a><br>
      <img src="../.github/assets/themes/cyberpunk.svg" alt="cyberpunk theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/vaporwave.toml"><b>vaporwave</b></a><br>
      <img src="../.github/assets/themes/vaporwave.svg" alt="vaporwave theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/matrix.toml"><b>matrix</b></a><br>
      <img src="../.github/assets/themes/matrix.svg" alt="matrix theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/tron.toml"><b>tron</b></a><br>
      <img src="../.github/assets/themes/tron.svg" alt="tron theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/eldritch.toml"><b>eldritch</b></a><br>
      <img src="../.github/assets/themes/eldritch.svg" alt="eldritch theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/hal.toml"><b>hal</b></a><br>
      <img src="../.github/assets/themes/hal.svg" alt="hal theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/gameboy.toml"><b>gameboy</b></a><br>
      <img src="../.github/assets/themes/gameboy.svg" alt="gameboy theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/wasteland.toml"><b>wasteland</b></a><br>
      <img src="../.github/assets/themes/wasteland.svg" alt="wasteland theme" width="100%">
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="../src/tui/themes/arcade.toml"><b>arcade</b></a><br>
      <img src="../.github/assets/themes/arcade.svg" alt="arcade theme" width="100%">
    </td>
    <td align="center" width="50%">
      <a href="../src/tui/themes/deep-space.toml"><b>deep-space</b></a><br>
      <img src="../.github/assets/themes/deep-space.svg" alt="deep-space theme" width="100%">
    </td>
  </tr>
</table>
