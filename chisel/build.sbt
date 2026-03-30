// Tourbillon Chisel backend — simulation tests
// Generated modules live in src/main/scala/tbn/
// Regenerate with: make generate

val chiselVersion = "7.0.0-RC1"

lazy val root = (project in file("."))
  .settings(
    name := "tbn-chisel",
    scalaVersion := "2.13.16",
    libraryDependencies ++= Seq(
      "org.chipsalliance" %% "chisel" % chiselVersion,
      "org.scalatest" %% "scalatest" % "3.2.19" % "test",
    ),
    addCompilerPlugin(
      "org.chipsalliance" % "chisel-plugin" % chiselVersion cross CrossVersion.full
    ),
    scalacOptions ++= Seq(
      "-language:reflectiveCalls",
      "-deprecation",
      "-feature",
    ),
  )
