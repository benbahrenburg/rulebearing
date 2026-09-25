#!/usr/bin/env node
/* eslint-disable */
// A minimal Plug'n'Play manifest in the shape `yarn install` writes: the runtime state as a JSON
// string. Only the data is read; nothing here runs.
"use strict";

const RAW_RUNTIME_STATE =
'{\
  "__info": [],\
  "dependencyTreeRoots": [{"name": "pnp-app", "reference": "workspace:."}],\
  "enableTopLevelFallback": true,\
  "ignorePatternData": null,\
  "fallbackExclusionList": [],\
  "fallbackPool": [],\
  "packageRegistryData": [\
    [null, [\
      [null, {\
        "packageLocation": "./",\
        "packageDependencies": [["left-pad", "npm:1.3.0"], ["pnp-app", "workspace:."]],\
        "linkType": "SOFT"\
      }]\
    ]],\
    ["left-pad", [\
      ["npm:1.3.0", {\
        "packageLocation": "./.yarn/unplugged/left-pad-npm-1.3.0/node_modules/left-pad/",\
        "packageDependencies": [["left-pad", "npm:1.3.0"]],\
        "linkType": "HARD"\
      }]\
    ]],\
    ["pnp-app", [\
      ["workspace:.", {\
        "packageLocation": "./",\
        "packageDependencies": [["left-pad", "npm:1.3.0"], ["pnp-app", "workspace:."]],\
        "linkType": "SOFT"\
      }]\
    ]]\
  ]\
}';

module.exports = { RAW_RUNTIME_STATE };
