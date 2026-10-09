<!-- Draft offer for abpframework/abp (plan 0003, Step 25; design § Open questions: withdrawn without argument if declined). Not posted. -->

# Proposal: architecture rules for abpframework/abp, generated from its current dependency graph

**Title:** Optional: a generated set of architecture rules for abpframework/abp

Hi! I maintain [Rulebearing](https://github.com/benbahrenburg/rulebearing), an open-source (MIT) tool that checks a repository's import graph against architecture rules, for TypeScript, .NET and Python in one rule file. As part of validating it against real projects, I ran its `init` command on abpframework/abp at `e402e7e30500`, and it proposed a rule set from what the repository already does. I'm sharing it in case it's useful; there is nothing to install unless you want to try it.

**What the proposal is.** 3 rules over .NET (the framework solution, built), extending `[rulebearing:dotnet, rulebearing:recommended]`. Each rule records a boundary the code already keeps, for example that a top-level package never imports the packages that already import it (which would make a cycle), so a change that crosses one is flagged in review. The rules derived from the code carry a comment saying why and a fix saying what to do. Existing violations (2805 at this commit) are baselined, so the check passes on today's code and fails only on new crossings. The full file is attached (`rulebearing.yaml`).

**Trying it** takes one command from the repository root, and changes nothing else:

```sh
npx rulebearing cruise --config rulebearing.yaml .
```

If this isn't something you want, please close the issue; I won't follow up. If you'd like the rules adjusted, or want to know how any one was derived, I'm happy to help.

<!-- Attach: testbeds/init/abpframework__abp/rulebearing.yaml -->
