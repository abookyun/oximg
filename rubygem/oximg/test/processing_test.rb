# frozen_string_literal: true

require "test_helper"

# End-to-end against a real binary: the argv tests pin what the gem
# says, these pin that the CLI still answers it. Skipped when no
# executable is around.
class Oximg::ProcessingTest < Oximg::Test
  def setup
    super
    require_executable!
  end

  def test_fits_within_the_box_without_enlarging
    in_tmpdir do |dir|
      out = File.join(dir, "out.jpg")
      assert_equal out, Oximg.resize(fixture("photo.jpg"), out, width: 100, height: 100)

      probed = Oximg.probe(out)
      assert_equal :jpeg, probed[:format]
      assert_operator probed[:width], :<=, 100
      assert_operator probed[:height], :<=, 100
    end
  end

  def test_a_zero_axis_is_unconstrained
    in_tmpdir do |dir|
      out = File.join(dir, "out.jpg")
      Oximg.resize(fixture("photo.jpg"), out, width: 100)
      assert_equal 100, Oximg.probe(out)[:width]
    end
  end

  def test_transcodes_by_destination_extension
    in_tmpdir do |dir|
      out = File.join(dir, "out.webp")
      Oximg.resize(fixture("photo.jpg"), out, width: 100)
      assert_equal :webp, Oximg.probe(out)[:format]
    end
  end

  def test_an_explicit_format_beats_the_extension
    in_tmpdir do |dir|
      out = File.join(dir, "out.bin")
      Oximg.resize(fixture("photo.jpg"), out, width: 100, format: :png)
      assert_equal :png, Oximg.probe(out)[:format]
    end
  end

  # The compression-only call: no resize, just a re-encode.
  def test_re_encodes_at_the_sources_own_size_by_default
    in_tmpdir do |dir|
      source = Oximg.probe(fixture("photo.jpg"))
      out = File.join(dir, "out.jpg")
      Oximg.resize(fixture("photo.jpg"), out, quality: 60)

      probed = Oximg.probe(out)
      assert_equal [source[:width], source[:height]], [probed[:width], probed[:height]]
      assert_operator File.size(out), :<, File.size(fixture("photo.jpg"))
    end
  end

  def test_probe_reads_headers_only
    probed = Oximg.probe(fixture("photo.jpg"))
    assert_equal "image/jpeg", probed[:content_type]
    assert_operator probed[:width], :>, 0
    assert_operator probed[:height], :>, 0
  end

  # The CLI appends ", 3 frames, 1500ms, looping forever" for an
  # animated source; the probe parser has to read past it rather than
  # give up on the line.
  def test_probe_reads_an_animated_source
    probed = Oximg.probe(fixture("anim.gif"))
    assert_equal "image/gif", probed[:content_type]
    assert_equal [120, 90], [probed[:width], probed[:height]]

    probed = Oximg.probe(fixture("animated.webp"))
    assert_equal "image/webp", probed[:content_type]
    assert_equal [64, 48], [probed[:width], probed[:height]]
  end

  # GIF is decode-only, but it is still a content type the CLI emits,
  # so probe has to name it.
  def test_probe_names_gif_as_a_format
    probed = Oximg.probe(fixture("still.gif"))
    assert_equal "image/gif", probed[:content_type]
    assert_equal :gif, probed[:format]
    assert_equal [240, 180], [probed[:width], probed[:height]]
  end

  # The guard on the parser itself. The run succeeds, but the gem does
  # not understand the output, so it must raise instead of returning a
  # half-read hash. Stubbed, because a working binary never prints
  # such a line.
  def test_probe_rejects_output_it_cannot_parse
    stub_run(["oximg 0.11.0\n", ""]) do
      error = assert_raises(Oximg::ProcessingError) { Oximg.probe(fixture("photo.jpg")) }
      assert_match(/unparsable probe output/, error.message)
    end
  end

  # The binary already names what it refused; the gem must surface that
  # rather than a bare exit status.
  def test_surfaces_the_binarys_own_error
    in_tmpdir do |dir|
      error = assert_raises(Oximg::ProcessingError) do
        Oximg.resize(File.join(dir, "missing.jpg"), File.join(dir, "out.jpg"), width: 100)
      end
      assert_match(/missing\.jpg/, error.message)
      refute_nil error.status
    end
  end

  private

  # A one-off singleton override rather than Minitest::Mock#stub:
  # minitest 6 dropped minitest/mock, and the gemspec allows ~> 5.0, so
  # a suite that reached for it would pass here and fail on 6.
  def stub_run(result)
    singleton = Oximg::Binary.singleton_class
    original = Oximg::Binary.method(:run)
    singleton.send(:remove_method, :run)
    Oximg::Binary.define_singleton_method(:run) { |*| result }
    yield
  ensure
    singleton.send(:remove_method, :run)
    singleton.send(:define_method, :run, original)
  end
end
