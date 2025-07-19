use std::time::Instant;

pub enum AnimationDirection {
    In,
    Out,
}

pub struct Animation {
    progress: f64,
    running: bool,
    last_update: Instant,
    step: f64,
    interval_ms: f64,
    direction: AnimationDirection,
}

impl Animation {
    pub fn new(step: f64, interval_ms: f64) -> Self {
        Self {
            progress: 0.0,
            running: false,
            last_update: Instant::now(),
            step,
            interval_ms,
            direction: AnimationDirection::In,
        }
    }

    pub fn animate_in(&mut self) {
        self.progress = 0.0;
        self.running = true;
        self.last_update = Instant::now();
        self.direction = AnimationDirection::In;
    }

    pub fn animate_out(&mut self) {
        self.progress = 1.0;
        self.running = true;
        self.last_update = Instant::now();
        self.direction = AnimationDirection::Out;
    }

    pub fn update(&mut self) -> bool {
        if !self.running { return false; }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_millis() as f64;
        if elapsed >= self.interval_ms {
            match self.direction {
                AnimationDirection::In => {
                    self.progress += self.step;
                    if self.progress >= 1.0 {
                        self.progress = 1.0;
                        self.running = false;
                    }
                }
                AnimationDirection::Out => {
                    self.progress -= self.step;
                    if self.progress <= 0.0 {
                        self.progress = 0.0;
                        self.running = false;
                    }
                }
            }
            self.last_update = now;
            return true;
        }
        false
    }

    pub fn is_animating_in(&self) -> bool {
        self.running && matches!(self.direction, AnimationDirection::In)
    }
    pub fn is_animating_out(&self) -> bool {
        self.running && matches!(self.direction, AnimationDirection::Out)
    }
    pub fn progress(&self) -> f64 { self.progress }
    pub fn is_running(&self) -> bool { self.running }
} 