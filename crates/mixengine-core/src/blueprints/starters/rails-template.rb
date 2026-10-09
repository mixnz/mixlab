# The Rails application template MixEngine's rails blueprint passes to `rails new -m`.
#
# It points development at the database MixEngine made, through DEVELOPMENT_DATABASE_URL in .env,
# and leaves every other environment as Rails wrote it: Rails merges DATABASE_URL into whichever
# environment runs, so under that name `bin/rails test` would purge the development database.

gsub_file "config/database.yml",
          /^(development:\n  <<: \*default\n)  database: .*\n/,
          "\\1  url: <%= ENV.fetch(\"DEVELOPMENT_DATABASE_URL\", \"postgres:///#{app_name}_development\") %>\n"

create_file "config/dotenv.rb", <<~'RUBY'
  # Reads .env beside config/ into ENV, without replacing a variable already set. Written by
  # MixEngine's rails blueprint; delete it and its require in config/boot.rb to read .env some
  # other way.
  path = File.expand_path("../.env", __dir__)
  if File.file?(path)
    File.foreach(path) do |line|
      line = line.strip.delete_prefix("export ").strip
      next if line.empty? || line.start_with?("#")

      key, value = line.split("=", 2)
      next if value.nil?

      ENV[key.strip] ||= value.strip.delete_prefix('"').delete_suffix('"')
    end
  end
RUBY

prepend_to_file "config/boot.rb", "require_relative \"dotenv\"\n"
