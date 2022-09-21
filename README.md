<p align=center>
  <img src="website/public/logo.svg" alt="ast-grep"/>
</p>

## ast-grep(sg)

ast-grep(sg) is a lightning fast and user friendly tool for code searching, linting, rewriting at large scale.

## Introduction
ast-grep is a AST-based tool to search code by pattern code. Think it as your old-friend `grep` but it matches AST node instead of text.
You can write pattern as if you are writing ordinary code. It will match all code that has the same syntactical structure.
You can use `$` sign + upper case letters as wildcard, e.g. `$MATCH`, to match any single AST node. Think it as REGEX dot `.`, except it is not textual.

Try the [online playground](https://ast-grep.github.io/ast-grep/playground.html) for a taste!

## Demo

![output](https://user-images.githubusercontent.com/2883231/183275066-8d9c342f-46cb-4fa5-aa4e-b98aac011869.gif)

## Installation
You can install it from [npm](https://docs.npmjs.com/downloading-and-installing-node-js-and-npm) or [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)!

```bash
# install via npm
npm i @ast-grep/cli -g

# install via cargo
cargo install ast-grep
```
Or you can build ast-grep from source. You need install rustup, clone the repository and then
```bash
cargo install --path ./crates/cli
```

## Command line usage example

ast-grep has following form.
```
sg --pattern 'var code = $PATTERN' --rewrite 'let code = new $PATTERN' --lang ts
```

### Example

* [Rewrite code in null coalescing operator](https://twitter.com/Hchan_mgn/status/1547061516993699841?s=20&t=ldDoj4U2nq-FRKQkU5GWXA)

```bash
sg -p '$A && $A()' -l ts -r '$A?.()'
```

* [Rewrite](https://twitter.com/Hchan_mgn/status/1561802312846278657) [Zodios](https://github.com/ecyrbe/zodios#migrate-to-v8)
```bash
sg -p 'new Zodios($URL,  $CONF as const,)' -l ts -r 'new Zodios($URL, $CONF)' -i
```

* [Implement eslint rule using YAML.](https://twitter.com/Hchan_mgn/status/1560108625460355073)


## Sponsor
![Sponsors](https://raw.githubusercontent.com/HerringtonDarkholme/sponsors/main/sponsorkit/sponsors.svg)

If you find ASTGrep interesting and useful for your work, please [buy me a coffee](https://github.com/sponsors/HerringtonDarkholme)
so I can spend more time on the project!


## Feature Highlight

ASTGrep's core is an algorithm to search and replace code based on abstract syntax tree produced by tree-sitter.
It can help you to do lightweight static analysis and massive scale code manipulation in an intuitive way.

Key highlights:

* An intuitive pattern to find and replace AST.
ASTGrep's pattern looks like ordinary code you would write every day. (You can call the pattern is isomorphic to code).

* jQuery like API for AST traversal and manipulatioin.

* YAML configuration to write new linting rules or code modification.

* Written in compiled language, parsing with tree-sitter and utilizing multiple cores.

* Beautiful command line interface :)

ast-grep's vision is to democratize abstract syntax tree magic and to liberate one from cumbersome AST programming!

* If you are an open source library author, ast-grep can help your library users adopt breaking changes more easily.
* if you are a tech lead in your team, ast-grep can help you enforce code best practice tailored to your business need.
* If you are a security researcher, ast-grep can helpn you write rules much faster.


## CLI Screenshot

### Search
| Feature | Command | Screenshot |
| ------- | ------- | ---------- |
| Search  | `sg -p 'Some($A)' -l rs` | ![image](https://user-images.githubusercontent.com/2883231/187094771-d60d1c19-98b8-4275-a040-5beca0074fd4.png) |
| Rewrite | `sg -p '$F && $F($$$ARGS)' -r '$F?.($ARGS)' -l ts` | ![image](https://user-images.githubusercontent.com/2883231/187094954-7143789d-c196-4576-ac39-906efac3a24a.png)|
| Report  | `sg scan` | ![image](https://user-images.githubusercontent.com/2883231/187094977-fd544d4b-64de-4bba-8bea-8c0de047b352.png) |



