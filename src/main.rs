use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Instant, SystemTime},
};

use bevy::{
    camera::visibility::NoFrustumCulling,
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin},
    prelude::*,
    render::view::NoIndirectDrawing,
};
use instancing::{CustomMaterialPlugin, InstanceData, InstanceMaterialData};
use rand::Rng;
use roto::{Runtime, TypedFunc, Val, library};

mod instancing;

fn main() {
    let mut args = std::env::args();
    let path = args.nth(1).expect("need a path to a script!");

    App::new()
        .insert_resource(ScriptManager::new(Path::new(&path)))
        .add_plugins((
            DefaultPlugins,
            CustomMaterialPlugin,
            FpsOverlayPlugin {
                config: FpsOverlayConfig::default(),
            },
        ))
        .add_systems(Startup, setup)
        .add_systems(Startup, time_in_roto_setup)
        .add_systems(
            FixedUpdate,
            (
                reload_script,
                add_particles,
                update_particles,
                update_instances,
                time_in_roto_update,
            ),
        )
        .run();
}

static EMITTER: Mutex<Vec<Particle>> = Mutex::new(Vec::new());

#[derive(Component)]
struct Particles(Vec<ParticleWithTime>);

struct ParticleWithTime {
    start_time: f32,
    particle: Particle,
}

#[derive(Clone, Debug)]
struct Particle {
    pos: Vec3,
    scale: f32,
    color: Color,
}

type UpdateFn = fn(f32, Val<Particle>) -> Option<Val<Particle>>;
type AddFn = fn(f32);

#[derive(Resource)]
struct ScriptManager {
    runtime: Runtime,
    path: PathBuf,
    last_compile: SystemTime,
    script_not_found_logged: bool,
    update: Option<TypedFunc<(), UpdateFn>>,
    update_ms: f32,
    add: Option<TypedFunc<(), AddFn>>,
    add_ms: f32,
}

impl ScriptManager {
    fn new(path: &Path) -> Self {
        let lib = library! {
            #[copy] type Vec3 = Val<Vec3>;
            #[copy] type Color = Val<Color>;
            #[clone] type Particle = Val<Particle>;

            fn emit(particle: Val<Particle>) {
                EMITTER.lock().unwrap().push(particle.0);
            }

            impl Val<Particle> {
                fn new(pos: Val<Vec3>, scale: f32, color: Val<Color>) -> Val<Particle> {
                    Val(Particle { pos: pos.0, scale, color: color.0 })
                }

                fn pos(p: Val<Particle>) -> Val<Vec3> {
                    Val(p.pos)
                }

                fn scale(p: Val<Particle>) -> f32 {
                    p.scale
                }

                fn color(p: Val<Particle>) -> Val<Color> {
                    Val(p.color)
                }
            }

            impl Val<Vec3> {
                fn new(x: f32, y: f32, z: f32) -> Val<Vec3> {
                    Val(Vec3 { x, y, z })
                }

                fn add(Val(x): Val<Vec3>, Val(y): Val<Vec3>) -> Val<Vec3> {
                    Val(x + y)
                }

                fn x(v: Val<Vec3>) -> f32 {
                    v.x
                }

                fn y(v: Val<Vec3>) -> f32 {
                    v.y
                }

                fn z(v: Val<Vec3>) -> f32 {
                    v.z
                }

                fn length(v: Val<Vec3>) -> f32 {
                    v.length()
                }

                fn normalize(Val(v): Val<Vec3>) -> Val<Vec3> {
                    Val(v.normalize())
                }

                fn scale(Val(v): Val<Vec3>, r: f32) -> Val<Vec3> {
                    Val(r * v)
                }
            }

            impl Val<Color> {
                fn red() -> Val<Color> {
                    Val(Color::from(Srgba::RED))
                }

                fn none() -> Val<Color> {
                    Val(Color::from(Srgba::NONE))
                }

                fn new(r: f32, g: f32, b: f32) -> Val<Color> {
                    Val(Color::from(Srgba::new(r, g, b, 1.0)))
                }

                fn mix(t: f32, Val(x): Val<Color>, Val(y): Val<Color>) -> Val<Color> {
                    Val(x.mix(&y, t))
                }
            }

            impl f32 {
                fn rand(low: f32, high: f32) -> f32 {
                    let mut rng = rand::rng();
                    rng.random_range(low..high)
                }

                fn sin(x: f32) -> f32 {
                    x.sin()
                }

                fn cos(x: f32) -> f32 {
                    x.cos()
                }

                fn pi() -> f32 {
                    std::f32::consts::PI
                }
            }
        };

        let mut runtime = Runtime::from_lib(lib).unwrap();
        runtime.add_io_functions();

        Self {
            runtime,
            path: path.to_path_buf(),
            last_compile: SystemTime::UNIX_EPOCH,
            script_not_found_logged: false,
            update: None,
            update_ms: 0.0,
            add: None,
            add_ms: 0.0,
        }
    }

    fn reload(&mut self) {
        let res = std::fs::metadata(&self.path);

        let modified = match res.and_then(|md| md.modified()) {
            Ok(modified) => modified,
            Err(e) => {
                self.last_compile = SystemTime::now();
                if !self.script_not_found_logged {
                    eprintln!("Script not found: {e}");
                    self.script_not_found_logged = true;
                }
                return;
            }
        };

        self.script_not_found_logged = false;

        if self.last_compile > modified {
            // We check this later than it was modified, just continue.
            return;
        }

        self.last_compile = SystemTime::now();

        let res = self.runtime.compile(&self.path);

        let mut pkg = match res {
            Ok(pkg) => pkg,
            Err(e) => {
                println!("{e}");
                return;
            }
        };

        if let Ok(update) = pkg.get_function("update") {
            self.update = Some(update);
        }

        if let Ok(add) = pkg.get_function("add") {
            self.add = Some(add);
        }
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(0.5))),
        InstanceMaterialData(Vec::new()),
        NoFrustumCulling,
    ));

    commands.spawn(Particles(Vec::new()));

    commands.spawn((
        Camera {
            clear_color: ClearColorConfig::Custom(Color::from(Srgba::rgb(0.0, 0.0, 0.05))),
            ..default()
        },
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        NoIndirectDrawing,
    ));
}

fn reload_script(mut manager: ResMut<ScriptManager>) {
    manager.reload();
}

fn add_particles(
    time: Res<Time>,
    mut manager: ResMut<ScriptManager>,
    mut particles: Single<&mut Particles>,
) {
    if let Some(add) = &manager.add {
        let t1 = Instant::now();
        add.call(&mut (), time.elapsed_secs());
        let t2 = Instant::now();
        let duration = t2 - t1;
        manager.add_ms = (duration.as_secs_f64() * 1000.0) as f32;
    } else {
        // let mut rng = rand::rng();
        // let x = rng.random_range(-10.0..10.0);
        // let y = rng.random_range(-10.0..10.0);

        // let particle = Particle {
        //     pos: Vec3 { x, y, z: 0. },
        //     scale: 1.0,
        //     color: Color::from(Srgba::RED),
        // };
        // EMITTER.lock().unwrap().push(particle);
    }

    let mut e = EMITTER.lock().unwrap();
    for particle in e.drain(..) {
        particles.0.push(ParticleWithTime {
            start_time: time.elapsed_secs(),
            particle: particle.clone(),
        });
    }
}

fn update_particles(
    mut manager: ResMut<ScriptManager>,
    time: Res<Time>,
    mut particles: Single<&mut Particles>,
) {
    let Some(update) = &manager.update else {
        return;
    };

    let t1 = Instant::now();
    particles.0.retain_mut(|p| {
        let t = time.elapsed_secs() - p.start_time;
        let particle = p.particle.clone();
        let res = update.call(&mut (), t, Val(particle));

        if let Some(Val(new)) = res {
            p.particle = new;
            true
        } else {
            false
        }
    });
    let t2 = Instant::now();
    let duration = t2 - t1;
    manager.update_ms = (duration.as_secs_f64() * 1000.0) as f32;
}

fn update_instances(
    particles: Single<&Particles>,
    mut instances: Single<&mut InstanceMaterialData>,
) {
    instances
        .0
        .resize(particles.0.len(), InstanceData::default());

    for (p, i) in particles.0.iter().zip(&mut instances.0) {
        i.position = p.particle.pos;
        i.scale = p.particle.scale;
        i.color = LinearRgba::from(p.particle.color).to_f32_array();
    }
}

#[derive(Component, Clone, Copy)]
enum TimeInRotoText {
    Add,
    Update,
    Particles,
}

fn time_in_roto_setup(mut commands: Commands) {
    commands
        .spawn((
            Node {
                // We need to make sure the overlay doesn't affect the position of other UI nodes
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                right: px(0),
                align_items: AlignItems::End,
                ..Default::default()
            },
            // Render overlay on top of everything
            GlobalZIndex(i32::MAX - 32),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("Add: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Add,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default())
            .with_child(TextSpan::new("ms"));
            p.spawn((
                Text::new("Update: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Update,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default())
            .with_child(TextSpan::new("ms"));
            p.spawn((
                Text::new("Particles: "),
                TextColor(Color::from(Srgba::WHITE)),
                TimeInRotoText::Particles,
                Pickable::IGNORE,
            ))
            .with_child(TextSpan::default());
        });
}

fn time_in_roto_update(
    query: Query<(Entity, &TimeInRotoText)>,
    mut writer: TextUiWriter,
    manager: Res<ScriptManager>,
    particles: Single<&Particles>,
) {
    for (entity, time_in_roto) in &query {
        match time_in_roto {
            TimeInRotoText::Add => {
                *writer.text(entity, 1) = format!("{:>6.2}", manager.add_ms);
            }
            TimeInRotoText::Update => {
                *writer.text(entity, 1) = format!("{:>6.2}", manager.update_ms);
            }
            TimeInRotoText::Particles => {
                *writer.text(entity, 1) = format!("{:>8}", particles.0.len());
            }
        }
    }
}
