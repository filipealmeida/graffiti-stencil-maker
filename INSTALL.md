# Installation

Step-by-step instructions to download, install and run **graffiti-stencil-maker** on Windows, Linux and macOS.
For how to use it afterwards, see [USAGE.md](USAGE.md).

## What you need

| Dependency | Version | Needed for |
|---|---|---|
| Git | any recent | downloading the project (or use the ZIP download) |
| Python | 3.10 or newer (3.12 tested) | everything |
| Node.js + npm | 18 or newer (22 tested) | building the studio web interface (one time) |
| Cairo library | system library | only for **SVG** input (`cairosvg`); PNG/JPG/BMP work without it |

The Python packages (`numpy`, `pillow`, `scipy`, `flask`, `cairosvg`, `manifold3d`, `fastapi`, `uvicorn`,
`scikit-learn`, `python-multipart`) are installed from [requirements.txt](requirements.txt).

You can skip Node.js if you only want the command line or the legacy web UI (`python -m stencil --serve`);
the node-graph studio needs it.

---

## Windows (10 / 11)

Use **PowerShell** for all commands.

### 1. Install Git, Python and Node.js

Either with `winget` (built into Windows 10/11):

```powershell
winget install --id Git.Git -e
winget install --id Python.Python.3.12 -e
winget install --id OpenJS.NodeJS.LTS -e
```

or download the installers from <https://git-scm.com/download/win>, <https://www.python.org/downloads/windows/>
(tick **"Add python.exe to PATH"** on the first screen) and <https://nodejs.org/>.

**Close and reopen PowerShell**, then check:

```powershell
git --version
python --version     # 3.10 or newer
node --version
npm --version
```

If `python` opens the Microsoft Store, disable the alias in *Settings → Apps → Advanced app settings → App execution aliases*,
or use `py -3` instead of `python` everywhere below.

### 2. Download the project

```powershell
cd $HOME
git clone https://github.com/filipealmeida/graffiti-stencil-maker.git
cd graffiti-stencil-maker
```

No Git? Open <https://github.com/filipealmeida/graffiti-stencil-maker>, click **Code → Download ZIP**, extract it and
open PowerShell in the extracted folder.

### 3. Create a virtual environment and install the Python packages

```powershell
python -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install --upgrade pip
pip install -r requirements.txt
```

If activation is blocked ("running scripts is disabled"), run once:
`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, then activate again.
(Alternatively skip activation and call `.\.venv\Scripts\python.exe` instead of `python`.)

### 4. Build the studio web interface (one time)

```powershell
cd app\web
npm install
npm run build
cd ..\..
```

### 5. Run it

Studio (node-graph interface):

```powershell
$env:PYTHONPATH = "."
python -m app --port 5056
```

Open <http://127.0.0.1:5056> in Firefox, Chrome or Edge.

Legacy single-page UI and the command line:

```powershell
python -m stencil --serve                       # http://127.0.0.1:5000
python -m stencil input.png -o out.stl --width 100
```

### 6. SVG input on Windows (optional)

`cairosvg` needs the Cairo DLL. Easiest: install the GTK3 runtime from
<https://github.com/tschoonj/GTK-for-Windows-Runtime-Environment-Installer/releases> (restart PowerShell afterwards).
Without it, PNG/JPG/BMP still work and SVG shows an error. As a workaround, export your SVG to PNG first.

---

## Linux

Commands are for Debian/Ubuntu; equivalents for Fedora and Arch are given too.

### 1. Install Git, Python, Node.js and Cairo

Debian / Ubuntu:

```bash
sudo apt update
sudo apt install -y git python3 python3-venv python3-pip libcairo2 nodejs npm
```

Fedora:

```bash
sudo dnf install -y git python3 python3-pip cairo nodejs npm
```

Arch:

```bash
sudo pacman -S --needed git python python-pip cairo nodejs npm
```

Check the versions:

```bash
python3 --version    # 3.10 or newer
node --version       # 18 or newer
```

If your distribution's Node.js is older than 18 (for example older Ubuntu LTS releases), install a current one with
[nvm](https://github.com/nvm-sh/nvm):

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
# open a new terminal, then:
nvm install --lts
```

### 2. Download the project

```bash
cd ~
git clone https://github.com/filipealmeida/graffiti-stencil-maker.git
cd graffiti-stencil-maker
```

### 3. Create a virtual environment and install the Python packages

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip
pip install -r requirements.txt
```

### 4. Build the studio web interface (one time)

```bash
cd app/web
npm install
npm run build
cd ../..
```

### 5. Run it

Studio:

```bash
PYTHONPATH=. python -m app --port 5056
```

Open <http://127.0.0.1:5056> in Firefox or Chrome.

Legacy UI and the command line:

```bash
python -m stencil --serve                       # http://127.0.0.1:5000
python -m stencil input.png -o out.stl --width 100
```

---

## macOS

### 1. Install Homebrew, then Git, Python, Node.js and Cairo

Install [Homebrew](https://brew.sh) if you do not have it (follow the one-line installer on its home page, and the
"Next steps" it prints to add `brew` to your PATH). Then:

```bash
brew install git python node cairo pkg-config
```

(`git` also comes with the Xcode command line tools: `xcode-select --install`.)

Check:

```bash
python3 --version    # 3.10 or newer
node --version
```

### 2. Download the project

```bash
cd ~
git clone https://github.com/filipealmeida/graffiti-stencil-maker.git
cd graffiti-stencil-maker
```

### 3. Create a virtual environment and install the Python packages

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip
pip install -r requirements.txt
```

On Apple Silicon, if SVG input later fails with `no library called "cairo" was found`, tell Python where Homebrew keeps it:

```bash
export DYLD_FALLBACK_LIBRARY_PATH="$(brew --prefix)/lib"
```

(add this line to `~/.zshrc` to make it permanent).

### 4. Build the studio web interface (one time)

```bash
cd app/web
npm install
npm run build
cd ../..
```

### 5. Run it

Studio:

```bash
PYTHONPATH=. python -m app --port 5056
```

Open <http://127.0.0.1:5056> in Safari, Firefox or Chrome.

Legacy UI and the command line:

```bash
python -m stencil --serve                       # http://127.0.0.1:5000
python -m stencil input.png -o out.stl --width 100
```

---

## Everyday use

Each time you open a new terminal, go to the project folder and activate the virtual environment again:

| | Activate |
|---|---|
| Windows (PowerShell) | `.\.venv\Scripts\Activate.ps1` |
| Linux / macOS | `source .venv/bin/activate` |

Then start the studio as in step 5. Stop it with `Ctrl+C`.

### Update to a newer version

```bash
git pull
pip install -r requirements.txt          # in the activated virtual environment
cd app/web && npm install && npm run build && cd ../..
```

(on Windows PowerShell use `cd app\web; npm install; npm run build; cd ..\..`).

### Run it on another machine or port

By default the studio only listens on `127.0.0.1` (your own computer). To reach it from other devices on your network:

```bash
python -m app --host 0.0.0.0 --port 5056
```

There is no login: only do this on a network you trust.

### Development mode (optional)

Run the backend with `python -m app` (port 5056) and, in another terminal, `npm run dev` inside `app/web`;
open <http://localhost:5173>, which proxies `/api` to the backend and reloads on source changes.

### Run the tests (optional)

```bash
pip install pytest
PYTHONPATH=. python -m pytest -q tests          # Windows PowerShell: $env:PYTHONPATH="."; python -m pytest -q tests
```

---

## Troubleshooting

| Problem | Fix |
|---|---|
| `python: command not found` (Linux/macOS) | use `python3`; after activating the venv `python` works |
| `No module named 'app'` | start from the project root with `PYTHONPATH=.` set (see step 5) |
| Studio page says it cannot find the web build / 404 on `/` | run step 4 (`npm install && npm run build` in `app/web`) |
| `pip install` fails building `scipy` / `numpy` | upgrade pip (`python -m pip install --upgrade pip`); use Python 3.10-3.13 |
| `manifold3d` fails to install | use a 64-bit Python 3.10-3.13; prebuilt wheels exist for Windows, Linux and macOS |
| SVG input fails with a Cairo error | install the Cairo library as described for your OS above |
| 3D preview is blank | the browser needs WebGL; enable hardware acceleration or try Firefox/Chrome. You can still download the STL |
| Port already in use | choose another: `--port 5057` |
