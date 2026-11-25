//! How the atmosphere pass records its dispatches.
//!
//! The pass is a dozen small kernels (the LUTs, the sky view, the aerial
//! volume, the clouds' weather and shadow). Unprofiled, they share one
//! compute pass, which is the cheapest way to record them. While the
//! profiler records GPU time ([`helio_core::PassContext::gpu_scopes_enabled`])
//! each one is its own compute pass bracketed by a named GPU scope, so
//! `profile_gpu` reports what each dispatch costs on the GPU, barriers
//! included, instead of one "Atmosphere" total. Measuring splits the pass and
//! costs a few extra pass boundaries; with the profiler off nothing differs.

use helio_core::cmd::{CommandRecorder, ComputeCmds};

/// The scope callback: `true` begins the named scope, `false` ends it.
pub(crate) type ScopeFn<'a> = dyn FnMut(&mut CommandRecorder<'_>, &'static str, bool) + 'a;

enum Mode<'a, 'r> {
    /// Every step records into this one pass.
    Fused(&'a mut ComputeCmds<'r>),
    /// Every step opens its own pass inside a GPU scope.
    Scoped {
        cmds: &'a mut CommandRecorder<'r>,
        scope: &'a mut ScopeFn<'a>,
    },
}

/// The dispatches of one pass, recorded fused or scoped (see the module docs).
pub(crate) struct Steps<'a, 'r> {
    mode: Mode<'a, 'r>,
}

impl<'a, 'r> Steps<'a, 'r> {
    pub(crate) fn fused(pass: &'a mut ComputeCmds<'r>) -> Self {
        Self {
            mode: Mode::Fused(pass),
        }
    }

    pub(crate) fn scoped(cmds: &'a mut CommandRecorder<'r>, scope: &'a mut ScopeFn<'a>) -> Self {
        Self {
            mode: Mode::Scoped { cmds, scope },
        }
    }

    /// Records one dispatch. `record` must set every bind group and pipeline
    /// it needs: a scoped step starts in a fresh pass.
    pub(crate) fn step(&mut self, name: &'static str, record: impl FnOnce(&mut ComputeCmds<'_>)) {
        match &mut self.mode {
            Mode::Fused(pass) => record(pass),
            Mode::Scoped { cmds, scope } => {
                scope(cmds, name, true);
                {
                    let mut pass = cmds.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some(name),
                        timestamp_writes: None,
                    });
                    record(&mut pass);
                }
                scope(cmds, name, false);
            }
        }
    }
}
