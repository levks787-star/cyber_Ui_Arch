use glam::{Mat4, Quat, Vec3};

const PITCH_MIN: f32 = -1.45;
const PITCH_MAX: f32 = 1.45;
const DIST_MIN: f32 = 1.6;
const DIST_MAX: f32 = 48.0;
const SMOOTH: f32 = 0.055;

/// Unity Scene View camera: yaw around world up, clamped pitch, optional roll.
/// Motion targets are smoothed with a critically damped spring.
#[derive(Clone, Debug)]
pub struct Camera {
    yaw: f32,
    pitch: f32,
    roll: f32,
    distance: f32,
    pivot: Vec3,
    syaw: f32,
    spitch: f32,
    sroll: f32,
    sdist: f32,
    spivot: Vec3,
    vyaw: f32,
    vpitch: f32,
    vroll: f32,
    vdist: f32,
    vpivot: Vec3,
    flying: bool,
    fly_pos: Vec3,
    sfly: Vec3,
    vfly: Vec3,
    fly_yaw: f32,
    fly_pitch: f32,
    sfyaw: f32,
    sfpitch: f32,
    vfyaw: f32,
    vfpitch: f32,
    fov: f32,
    home_yaw: f32,
    home_pitch: f32,
    home_dist: f32,
    home_pivot: Vec3,
}

impl Camera {
    pub fn new() -> Self {
        let yaw = -0.22;
        let pitch = -0.30;
        let distance = 7.6;
        let pivot = Vec3::new(0.0, 0.05, 0.0);
        Self {
            yaw,
            pitch,
            roll: 0.0,
            distance,
            pivot,
            syaw: yaw,
            spitch: pitch,
            sroll: 0.0,
            sdist: distance,
            spivot: pivot,
            vyaw: 0.0,
            vpitch: 0.0,
            vroll: 0.0,
            vdist: 0.0,
            vpivot: Vec3::ZERO,
            flying: false,
            fly_pos: Vec3::ZERO,
            sfly: Vec3::ZERO,
            vfly: Vec3::ZERO,
            fly_yaw: yaw,
            fly_pitch: pitch,
            sfyaw: yaw,
            sfpitch: pitch,
            vfyaw: 0.0,
            vfpitch: 0.0,
            fov: 46_f32.to_radians(),
            home_yaw: yaw,
            home_pitch: pitch,
            home_dist: distance,
            home_pivot: pivot,
        }
    }

    fn orient(yaw: f32, pitch: f32, roll: f32) -> Quat {
        Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(roll)
    }

    fn pose(&self) -> (Quat, Vec3) {
        if self.flying {
            (Self::orient(self.sfyaw, self.sfpitch, 0.0), self.sfly)
        } else {
            (
                Self::orient(self.syaw, self.spitch, self.sroll),
                self.spivot + Self::orient(self.syaw, self.spitch, self.sroll) * Vec3::new(0.0, 0.0, self.sdist),
            )
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pose().1
    }

    fn forward_of(q: Quat) -> Vec3 {
        -(q * Vec3::Z)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let (q, eye) = self.pose();
        let target = if self.flying {
            eye + Self::forward_of(q)
        } else {
            self.spivot
        };
        let up = (q * Vec3::Y).normalize();
        let view = Mat4::look_at_rh(eye, target, up);
        let proj = Mat4::perspective_rh(self.fov, aspect.max(0.2), 0.08, 220.0);
        proj * view
    }

    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (Vec3, Vec3) {
        let (q, eye) = self.pose();
        let forward = if self.flying {
            Self::forward_of(q)
        } else {
            (self.spivot - eye).normalize_or_zero()
        };
        let forward = if forward.length_squared() < 1e-8 { Self::forward_of(q) } else { forward };
        let right = forward.cross((q * Vec3::Y).normalize()).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let half_h = (self.fov * 0.5).tan();
        let half_w = half_h * aspect.max(0.2);
        let dir = (forward + right * (nx * half_w) + up * (ny * half_h)).normalize_or_zero();
        (eye, if dir.length_squared() < 1e-8 { forward } else { dir })
    }

    pub fn orbit_pixels(&mut self, dx: f32, dy: f32) {
        self.leave_fly();
        let sens = 0.0052;
        self.yaw -= dx * sens;
        self.pitch = (self.pitch - dy * sens).clamp(PITCH_MIN, PITCH_MAX);
    }

    pub fn roll_pixels(&mut self, dx: f32) {
        self.leave_fly();
        self.roll += dx * 0.0052;
    }

    pub fn pan_pixels(&mut self, dx: f32, dy: f32) {
        self.leave_fly();
        let (q, _) = self.pose();
        let forward = (self.spivot - self.eye()).normalize_or_zero();
        let up_axis = (q * Vec3::Y).normalize_or_zero();
        let right = forward.cross(up_axis).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let scale = self.sdist * 0.00115;
        self.pivot -= right * dx * scale + up * dy * scale;
    }

    pub fn dolly_lines(&mut self, lines: f32) {
        self.leave_fly();
        self.distance = (self.distance * (-lines * 0.16).exp()).clamp(DIST_MIN, DIST_MAX);
    }

    pub fn dolly_pixels(&mut self, dy: f32) {
        self.dolly_lines(-dy * 0.012);
    }

    pub fn zoom_toward_cursor(&mut self, lines: f32, nx: f32, ny: f32, aspect: f32) {
        if lines.abs() < 1e-5 {
            return;
        }
        if self.flying {
            let (eye, dir) = self.ray(nx, ny, aspect);
            let step = lines * self.sdist.max(1.2) * 0.18;
            self.fly_pos = eye + dir * step;
            return;
        }
        let old = self.distance.max(0.001);
        let new_d = (old * (-lines * 0.16).exp()).clamp(DIST_MIN, DIST_MAX);
        let (eye, dir) = self.ray(nx, ny, aspect);
        let to_pivot = self.spivot - eye;
        let depth = to_pivot.length().max(0.05);
        let forward = to_pivot / depth;
        let denom = dir.dot(forward);
        if denom > 1e-3 {
            let hit = eye + dir * (depth / denom);
            let k = (1.0 - new_d / old).clamp(-1.2, 1.2);
            self.pivot += (hit - self.pivot) * k;
        }
        self.distance = new_d;
    }

    pub fn begin_fly(&mut self) {
        if self.flying {
            return;
        }
        let eye = self.eye();
        self.fly_pos = eye;
        self.sfly = eye;
        self.vfly = Vec3::ZERO;
        self.fly_yaw = self.syaw;
        self.fly_pitch = self.spitch;
        self.sfyaw = self.syaw;
        self.sfpitch = self.spitch;
        self.vfyaw = 0.0;
        self.vfpitch = 0.0;
        self.flying = true;
    }

    pub fn end_fly(&mut self) {
        self.leave_fly();
    }

    fn leave_fly(&mut self) {
        if !self.flying {
            return;
        }
        let q = Self::orient(self.sfyaw, self.sfpitch, self.sroll);
        let forward = Self::forward_of(q);
        self.yaw = self.sfyaw;
        self.pitch = self.sfpitch.clamp(PITCH_MIN, PITCH_MAX);
        self.syaw = self.yaw;
        self.spitch = self.pitch;
        self.vyaw = 0.0;
        self.vpitch = 0.0;
        self.distance = self.sdist.clamp(DIST_MIN, DIST_MAX);
        self.sdist = self.distance;
        self.vdist = 0.0;
        self.pivot = self.sfly + forward * self.distance;
        self.spivot = self.pivot;
        self.vpivot = Vec3::ZERO;
        self.flying = false;
    }

    pub fn fly_look(&mut self, dx: f32, dy: f32) {
        self.begin_fly();
        let sens = 0.0032;
        self.fly_yaw -= dx * sens;
        self.fly_pitch = (self.fly_pitch - dy * sens).clamp(PITCH_MIN, PITCH_MAX);
    }

    /// `wish` is camera-local: x right, y up, z forward.
    pub fn fly_move(&mut self, wish: Vec3, dt: f32, fast: bool) {
        if wish.length_squared() < 1e-8 || dt <= 0.0 {
            return;
        }
        self.begin_fly();
        let wish = wish.normalize();
        let q = Self::orient(self.sfyaw, self.sfpitch, 0.0);
        let forward = Self::forward_of(q);
        let right = q * Vec3::X;
        let speed = if fast { 16.0 } else { 5.0 };
        self.fly_pos += (right * wish.x + Vec3::Y * wish.y + forward * wish.z) * speed * dt;
    }

    pub fn frame_at(&mut self, point: Vec3, radius: f32) {
        self.leave_fly();
        self.pivot = point;
        let dist = (radius.max(0.4) / (self.fov * 0.5).tan() * 1.18).clamp(DIST_MIN, DIST_MAX);
        self.distance = dist;
    }

    pub fn request_reset(&mut self) {
        self.leave_fly();
        self.yaw = self.home_yaw;
        self.pitch = self.home_pitch;
        self.roll = 0.0;
        self.distance = self.home_dist;
        self.pivot = self.home_pivot;
    }

    pub fn tick(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        if dt <= 0.0 {
            return;
        }
        if self.flying {
            self.sfyaw = damp_angle(&mut self.vfyaw, self.sfyaw, self.fly_yaw, SMOOTH, dt);
            self.sfpitch = smooth_damp(self.sfpitch, self.fly_pitch, &mut self.vfpitch, SMOOTH, dt);
            self.sfly = smooth_damp_vec(self.sfly, self.fly_pos, &mut self.vfly, 0.07, dt);
            return;
        }
        self.syaw = damp_angle(&mut self.vyaw, self.syaw, self.yaw, SMOOTH, dt);
        self.spitch = smooth_damp(self.spitch, self.pitch, &mut self.vpitch, SMOOTH, dt).clamp(PITCH_MIN, PITCH_MAX);
        self.sroll = damp_angle(&mut self.vroll, self.sroll, self.roll, SMOOTH, dt);
        self.sdist = smooth_damp(self.sdist, self.distance, &mut self.vdist, 0.07, dt).clamp(DIST_MIN, DIST_MAX);
        self.spivot = smooth_damp_vec(self.spivot, self.pivot, &mut self.vpivot, 0.07, dt);
    }

    pub fn showcase_spin(&mut self, dt: f32) {
        if self.flying {
            return;
        }
        self.yaw += 0.05 * dt;
        self.syaw = self.yaw;
        self.vyaw = 0.0;
    }
}

fn smooth_damp(current: f32, target: f32, vel: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let smooth_time = smooth_time.max(1.0e-4);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * exp;
    target + (change + temp) * exp
}

fn smooth_damp_vec(current: Vec3, target: Vec3, vel: &mut Vec3, smooth_time: f32, dt: f32) -> Vec3 {
    Vec3::new(
        smooth_damp(current.x, target.x, &mut vel.x, smooth_time, dt),
        smooth_damp(current.y, target.y, &mut vel.y, smooth_time, dt),
        smooth_damp(current.z, target.z, &mut vel.z, smooth_time, dt),
    )
}

fn damp_angle(vel: &mut f32, current: f32, target: f32, smooth_time: f32, dt: f32) -> f32 {
    let mut delta = (target - current) % std::f32::consts::TAU;
    if delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    } else if delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    smooth_damp(current, current + delta, vel, smooth_time, dt)
}

pub fn damp(current: f32, target: f32, speed: f32, dt: f32) -> f32 {
    let t = 1.0 - (-speed * dt).exp();
    current + (target - current) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_reaches_target_without_blowup() {
        let mut vel = 0.0;
        let mut current = 0.0;
        for _ in 0..240 {
            current = smooth_damp(current, 1.0, &mut vel, 0.08, 1.0 / 144.0);
            assert!(current.is_finite());
        }
        assert!((current - 1.0).abs() < 0.02, "{current}");
    }

    #[test]
    fn reset_returns_home() {
        let home = Camera::new().eye();
        let mut cam = Camera::new();
        cam.orbit_pixels(400.0, -800.0);
        cam.dolly_lines(4.0);
        cam.roll_pixels(200.0);
        cam.request_reset();
        for _ in 0..240 {
            cam.tick(1.0 / 60.0);
        }
        let err = (cam.eye() - home).length();
        assert!(err < 0.05, "{err}");
    }
}
