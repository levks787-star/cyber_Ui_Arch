use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Debug)]
pub struct Orbit {
    pub rotation: Quat,
    pub distance: f32,
    pub target: Vec3,
    pub ang_vel: Vec3,
    pub fov: f32,
    pub home_rot: Quat,
    pub home_dist: f32,
    pub resetting: bool,
}

impl Orbit {
    pub fn new() -> Self {
        let pitch = Quat::from_rotation_x(-0.46);
        let yaw = Quat::from_rotation_y(-0.38);
        let home_rot = yaw * pitch;
        Self {
            rotation: home_rot,
            distance: 10.4,
            target: Vec3::new(0.0, -0.15, 0.0),
            ang_vel: Vec3::ZERO,
            fov: 48_f32.to_radians(),
            home_rot,
            home_dist: 10.4,
            resetting: false,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.rotation * Vec3::new(0.0, 0.0, self.distance)
    }

    pub fn up(&self) -> Vec3 {
        (self.rotation * Vec3::Y).normalize()
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.target, self.up());
        let proj = Mat4::perspective_rh(self.fov, aspect.max(0.2), 0.08, 220.0);
        proj * view
    }

    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (Vec3, Vec3) {
        let eye = self.eye();
        let forward = (self.target - eye).normalize();
        let right = forward.cross(self.up()).normalize();
        let up = right.cross(forward).normalize();
        let half_h = (self.fov * 0.5).tan();
        let half_w = half_h * aspect.max(0.2);
        let dir = (forward + right * (nx * half_w) + up * (ny * half_h)).normalize();
        (eye, dir)
    }

    pub fn zoom(&mut self, lines: f32) {
        self.resetting = false;
        self.distance = (self.distance * (-lines * 0.14).exp()).clamp(4.4, 26.0);
    }

    pub fn apply_trackball(&mut self, prev: (f32, f32), curr: (f32, f32), dt: f32) {
        self.resetting = false;
        let a = track_vec(prev.0, prev.1);
        let b = track_vec(curr.0, curr.1);
        let axis = a.cross(b);
        let dot = a.dot(b).clamp(-1.0, 1.0);
        let angle = dot.acos();
        if axis.length_squared() < 1e-8 || !angle.is_finite() || angle < 1e-5 {
            return;
        }
        let axis_n = axis.normalize();
        let q = Quat::from_axis_angle(axis_n, angle);
        self.rotation = (q * self.rotation).normalize();
        let dt = dt.max(1.0 / 240.0);
        self.ang_vel = axis_n * (angle / dt);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.resetting = false;
        let eye = self.eye();
        let forward = (self.target - eye).normalize();
        let right = forward.cross(self.up()).normalize();
        let up = right.cross(forward).normalize();
        let scale = self.distance * 0.00115;
        self.target += -right * dx * scale - up * dy * scale;
    }

    pub fn request_reset(&mut self) {
        self.resetting = true;
        self.ang_vel = Vec3::ZERO;
    }

    pub fn tick(&mut self, dt: f32) {
        if self.resetting {
            let t = 1.0 - (-5.0 * dt).exp();
            self.rotation = self.rotation.slerp(self.home_rot, t).normalize();
            self.distance += (self.home_dist - self.distance) * t;
            let home_target = Vec3::new(0.0, -0.15, 0.0);
            self.target += (home_target - self.target) * t;
            let delta = self.rotation.dot(self.home_rot).abs();
            if delta > 0.9995 && (self.distance - self.home_dist).abs() < 0.03 {
                self.rotation = self.home_rot;
                self.distance = self.home_dist;
                self.target = home_target;
                self.resetting = false;
            }
            return;
        }
        let speed = self.ang_vel.length();
        if speed > 0.01 {
            let axis = self.ang_vel / speed;
            let q = Quat::from_axis_angle(axis, speed * dt);
            self.rotation = (q * self.rotation).normalize();
            self.ang_vel *= (-2.6 * dt).exp();
        } else {
            self.ang_vel = Vec3::ZERO;
        }
    }

    pub fn showcase_spin(&mut self, dt: f32) {
        if self.resetting {
            return;
        }
        let q = Quat::from_axis_angle(Vec3::Y, 0.22 * dt);
        self.rotation = (q * self.rotation).normalize();
    }
}

fn track_vec(nx: f32, ny: f32) -> Vec3 {
    let d = nx * nx + ny * ny;
    if d <= 1.0 {
        Vec3::new(nx, ny, (1.0 - d).sqrt())
    } else {
        Vec3::new(nx, ny, 0.0).normalize()
    }
}

pub fn damp(current: f32, target: f32, speed: f32, dt: f32) -> f32 {
    let t = 1.0 - (-speed * dt).exp();
    current + (target - current) * t
}
