// generated from bayt.cue — do not edit
package boxer

import bayt_ "bonisoft.org/plugins/bayt/core:bayt"

depManifestsIn: {[string]: _}
runtimeIn: *"" | string
_render: (bayt_.#render & {"project": project, depManifests: depManifestsIn, runtime: runtimeIn})
